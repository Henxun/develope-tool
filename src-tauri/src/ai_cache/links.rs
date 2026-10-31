//! 链接类目标判定。
//!
//! Rust 的 `FileType::is_symlink()` 在 Windows 上不识别目录 junction
//! （`IO_REPARSE_TAG_MOUNT_POINT` reparse point）：junction 会被当成普通目录，
//! 扫描会跟进去算体积，`remove_dir_all` 对它的行为也未定义。
//!
//! 这里把 **符号链接 + Windows junction / symlink reparse point** 统一视为
//! "链接类"，与既有 `is_symlink()` 判断同权：扫描不进入、不计体积，清理拒绝。
//! 非 Windows 平台保持与 `is_symlink()` 完全一致的行为（空实现，零额外 syscall）。

use std::fs;
use std::path::Path;

/// 路径自身是否为链接类（不 follow 最终组件）。读取失败按"不是链接"处理。
pub fn is_link_like(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    metadata_is_link_like(&metadata, path)
}

/// 已拿到 `symlink_metadata` 时的链接类判断（避免重复 stat）。
pub fn metadata_is_link_like(metadata: &fs::Metadata, path: &Path) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        windows::is_reparse_link(metadata, path)
    }
    #[cfg(not(windows))]
    {
        let _ = (metadata, path);
        false
    }
}

/// `WalkDir` 条目判断。
///
/// 非 Windows 只认 symlink（与原来一致，不额外 stat）；Windows 对目录条目
/// 额外查询 reparse tag，避免 walkdir 跟进 junction 后把目标内容算进体积。
pub fn entry_is_link_like(file_type: fs::FileType, path: &Path) -> bool {
    if file_type.is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        file_type.is_dir() && windows::is_reparse_link_path(path)
    }
    #[cfg(not(windows))]
    {
        let _ = (file_type, path);
        false
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::fs;
    use std::mem;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::MetadataExt;
    use std::path::Path;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FileAttributeTagInfo, GetFileInformationByHandleEx,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::SystemServices::{
        IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK,
    };

    /// 读取 reparse tag（`FILE_ATTRIBUTE_TAG_INFO`）；失败返回 `None`。
    fn reparse_tag(path: &Path) -> Option<u32> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);

        // SAFETY: `wide` 以 NUL 结尾且在调用期间存活；句柄在同一个 unsafe 块内关闭。
        unsafe {
            let handle = CreateFileW(
                wide.as_ptr(),
                FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return None;
            }

            let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
            let ok = GetFileInformationByHandleEx(
                handle,
                FileAttributeTagInfo,
                &mut info as *mut FILE_ATTRIBUTE_TAG_INFO as *mut c_void,
                mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            );
            let _ = CloseHandle(handle);

            if ok == 0 {
                None
            } else {
                Some(info.ReparseTag)
            }
        }
    }

    fn has_reparse_attribute(metadata: &fs::Metadata) -> bool {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    fn is_link_tag(tag: u32) -> bool {
        tag == IO_REPARSE_TAG_MOUNT_POINT || tag == IO_REPARSE_TAG_SYMLINK
    }

    /// Windows：属性带 REPARSE_POINT 且 tag 为 junction / symlink 才算链接类。
    pub(super) fn is_reparse_link(metadata: &fs::Metadata, path: &Path) -> bool {
        if !has_reparse_attribute(metadata) {
            return false;
        }
        // 读不到 tag 时保守按链接类处理：宁可少清理，也不误删 junction 目标。
        reparse_tag(path).is_none_or(is_link_tag)
    }

    pub(super) fn is_reparse_link_path(path: &Path) -> bool {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return false;
        };
        is_reparse_link(&metadata, path)
    }
}
