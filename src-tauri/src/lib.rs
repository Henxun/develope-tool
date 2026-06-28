use bzip2::{read::BzDecoder, write::BzEncoder, Compression as BzCompression};
use flate2::{read::GzDecoder, write::GzEncoder, Compression as GzCompression};
use globset::{Glob, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
use tar::{Archive as TarArchive, Builder as TarBuilder};
use tauri::Emitter;
use uuid::Uuid;
use walkdir::WalkDir;
use xz2::{read::XzDecoder, write::XzEncoder};
use zip::{write::FileOptions, CompressionMethod, ZipArchive, ZipWriter};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND},
    Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};
#[cfg(windows)]
use winreg::{
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE},
    RegKey, RegValue,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemInfo {
    os: String,
    arch: String,
    current_dir: String,
    node_env: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveResult {
    output_path: String,
    processed_count: usize,
    skipped_count: usize,
    format: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgramMigrationRequest {
    app_name: String,
    source_dir: String,
    target_dir: String,
    additional_registry_keys: Vec<String>,
    additional_shortcut_dirs: Vec<String>,
    env_var_names: Vec<String>,
    #[serde(default)]
    include_machine_registry: bool,
    dry_run: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgramMigrationResult {
    app_name: String,
    source_dir: String,
    target_dir: String,
    moved: bool,
    registry_updates: usize,
    shortcut_updates: usize,
    env_var_updates: usize,
    warnings: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgramDetectRequest {
    app_name: String,
    source_dir: String,
    additional_shortcut_dirs: Vec<String>,
    env_var_names: Vec<String>,
    include_machine_registry: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgramDetectResult {
    normalized_source_dir: String,
    registry_keys: Vec<String>,
    shortcut_files: Vec<String>,
    env_var_matches: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToolDataMigrationRequest {
    tool_name: String,
    source_dir: String,
    target_dir: String,
    dry_run: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolDataMigrationResult {
    tool_name: String,
    source_dir: String,
    target_dir: String,
    moved: bool,
    symlink_created: bool,
    warnings: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolMigrationLogEvent {
    level: String,
    message: String,
    timestamp: String,
}

// ── Disk heatmap scan structures ──

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskNode {
    name: String,
    path: String,
    size: u64,
    node_type: String,
    modified_secs: u64,
    children: Vec<DiskNode>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskScanResult {
    root: DiskNode,
    total_size: u64,
    total_files: u64,
    total_dirs: u64,
    scan_duration_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileActionResult {
    success: bool,
    message: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskScanProgressEvent {
    current_path: String,
    items_scanned: u64,
}

#[derive(Clone, Copy)]
enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
}

impl ArchiveFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
            Self::TarBz2 => "tar.bz2",
            Self::TarXz => "tar.xz",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "zip" => Ok(Self::Zip),
            "tar" => Ok(Self::Tar),
            "tar.gz" | "tgz" => Ok(Self::TarGz),
            "tar.bz2" | "tbz2" => Ok(Self::TarBz2),
            "tar.xz" | "txz" => Ok(Self::TarXz),
            other => Err(format!("不支持的压缩格式: {other}")),
        }
    }

    fn from_archive_path(path: &Path) -> Result<Self, String> {
        let name = path
            .file_name()
            .ok_or_else(|| "无效压缩包路径".to_string())?
            .to_string_lossy()
            .to_ascii_lowercase();

        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            return Ok(Self::TarGz);
        }
        if name.ends_with(".tar.bz2") || name.ends_with(".tbz2") {
            return Ok(Self::TarBz2);
        }
        if name.ends_with(".tar.xz") || name.ends_with(".txz") {
            return Ok(Self::TarXz);
        }
        if name.ends_with(".tar") {
            return Ok(Self::Tar);
        }
        if name.ends_with(".zip") {
            return Ok(Self::Zip);
        }

        Err("无法根据文件扩展名识别压缩格式".to_string())
    }
}

#[tauri::command]
fn get_system_info() -> Result<SystemInfo, String> {
    let current_dir = env::current_dir()
        .map_err(|error| format!("无法读取当前目录: {error}"))?
        .display()
        .to_string();

    Ok(SystemInfo {
        os: env::consts::OS.to_string(),
        arch: env::consts::ARCH.to_string(),
        current_dir,
        node_env: env::var("NODE_ENV").unwrap_or_else(|_| "development".into()),
    })
}

#[tauri::command]
fn list_directory(path: Option<String>) -> Result<Vec<FileEntry>, String> {
    let target = match path {
        Some(value) if !value.trim().is_empty() => value,
        _ => env::current_dir()
            .map_err(|error| format!("无法读取当前目录: {error}"))?
            .display()
            .to_string(),
    };

    let mut entries: Vec<FileEntry> = fs::read_dir(&target)
        .map_err(|error| format!("读取目录失败: {error}"))?
        .filter_map(Result::ok)
        .take(200)
        .map(|entry| {
            let metadata = entry.metadata().ok();
            FileEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry.path().display().to_string(),
                is_dir: metadata.map(|data| data.is_dir()).unwrap_or(false),
            }
        })
        .collect();

    entries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    Ok(entries)
}

#[tauri::command]
fn compress_archive(
    source_path: String,
    output_path: String,
    format: String,
    excludes: Vec<String>,
) -> Result<ArchiveResult, String> {
    let source = PathBuf::from(source_path.trim());
    if !source.exists() {
        return Err("源路径不存在".to_string());
    }

    let output = PathBuf::from(output_path.trim());
    if output.as_os_str().is_empty() {
        return Err("输出路径不能为空".to_string());
    }

    let format = ArchiveFormat::parse(&format)?;
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| format!("创建输出目录失败: {error}"))?;
        }
    }

    let exclude_set = build_exclude_set(&excludes)?;
    let files = collect_files_for_archive(&source, &exclude_set)?;

    if files.is_empty() {
        return Err("没有可压缩的文件，请检查过滤条件".to_string());
    }

    match format {
        ArchiveFormat::Zip => compress_to_zip(&files, &output)?,
        ArchiveFormat::Tar => compress_to_tar(&files, &output)?,
        ArchiveFormat::TarGz => compress_to_targz(&files, &output)?,
        ArchiveFormat::TarBz2 => compress_to_tarbz2(&files, &output)?,
        ArchiveFormat::TarXz => compress_to_tarxz(&files, &output)?,
    }

    let skipped = count_skipped_entries(&source, &exclude_set);
    Ok(ArchiveResult {
        output_path: output.display().to_string(),
        processed_count: files.len(),
        skipped_count: skipped,
        format: format.as_str().to_string(),
    })
}

#[tauri::command]
fn extract_archive(
    archive_path: String,
    output_dir: String,
    excludes: Vec<String>,
) -> Result<ArchiveResult, String> {
    let archive = PathBuf::from(archive_path.trim());
    if !archive.exists() {
        return Err("压缩包路径不存在".to_string());
    }

    let output = PathBuf::from(output_dir.trim());
    if output.as_os_str().is_empty() {
        return Err("解压目录不能为空".to_string());
    }
    fs::create_dir_all(&output).map_err(|error| format!("创建解压目录失败: {error}"))?;

    let format = ArchiveFormat::from_archive_path(&archive)?;
    let exclude_set = build_exclude_set(&excludes)?;

    let (processed_count, skipped_count) = match format {
        ArchiveFormat::Zip => extract_zip(&archive, &output, &exclude_set)?,
        ArchiveFormat::Tar => extract_tar(&archive, &output, &exclude_set)?,
        ArchiveFormat::TarGz => extract_targz(&archive, &output, &exclude_set)?,
        ArchiveFormat::TarBz2 => extract_tarbz2(&archive, &output, &exclude_set)?,
        ArchiveFormat::TarXz => extract_tarxz(&archive, &output, &exclude_set)?,
    };

    Ok(ArchiveResult {
        output_path: output.display().to_string(),
        processed_count,
        skipped_count,
        format: format.as_str().to_string(),
    })
}

#[tauri::command]
fn migrate_installed_program(
    request: ProgramMigrationRequest,
) -> Result<ProgramMigrationResult, String> {
    #[cfg(not(windows))]
    {
        let _ = request;
        return Err("程序迁移工具仅支持 Windows 平台".to_string());
    }

    #[cfg(windows)]
    {
        migrate_installed_program_windows(request)
    }
}

#[tauri::command]
fn is_process_elevated() -> Result<bool, String> {
    #[cfg(not(windows))]
    {
        Ok(false)
    }

    #[cfg(windows)]
    {
        is_process_elevated_windows()
    }
}

#[tauri::command]
fn relaunch_as_admin() -> Result<(), String> {
    #[cfg(not(windows))]
    {
        Err("仅 Windows 支持提权重启".to_string())
    }

    #[cfg(windows)]
    {
        relaunch_as_admin_windows()
    }
}

#[tauri::command]
fn detect_program_references(request: ProgramDetectRequest) -> Result<ProgramDetectResult, String> {
    #[cfg(not(windows))]
    {
        let _ = request;
        return Err("程序检测仅支持 Windows 平台".to_string());
    }

    #[cfg(windows)]
    {
        detect_program_references_windows(request)
    }
}

#[tauri::command]
fn check_directory_locked(dir_path: String) -> Result<Vec<String>, String> {
    #[cfg(not(windows))]
    {
        let _ = dir_path;
        return Err("目录占用检测仅支持 Windows 平台".to_string());
    }

    #[cfg(windows)]
    {
        let path = dir_path.trim();
        if path.is_empty() {
            return Err("路径不能为空".to_string());
        }
        let source = PathBuf::from(path);
        if !source.is_absolute() {
            return Err("路径必须是绝对路径".to_string());
        }
        if !source.is_dir() {
            return Err("路径必须是目录".to_string());
        }

        check_directory_locked_windows(&source)
    }
}

#[cfg(windows)]
fn check_directory_locked_windows(source: &Path) -> Result<Vec<String>, String> {
    // Try rename the directory to a temp name and back.
    // On Windows, if ANY file or subdirectory inside is held open by another process,
    // or if the directory itself is a working directory of a process,
    // fs::rename will fail with PermissionDenied / SharingViolation.
    let temp_name = source.with_extension(format!("{}.locktest", Uuid::new_v4()));
    match fs::rename(source, &temp_name) {
        Ok(_) => match fs::rename(&temp_name, source) {
            Ok(_) => Ok(Vec::new()),
            Err(e) => Err(format!(
                "目录锁定检测失败: 无法将临时目录从 {} 恢复到原位置 {}: {}\n\
                恢复提示: 请手动将 {} 重命名回 {}",
                temp_name.display(),
                source.display(),
                e,
                temp_name.display(),
                source.display()
            )),
        },
        Err(e) => Ok(vec![format!("{} (目录被占用: {})", source.display(), e)]),
    }
}

#[tauri::command]
fn migrate_tool_data(
    window: tauri::Window,
    request: ToolDataMigrationRequest,
) -> Result<ToolDataMigrationResult, String> {
    #[cfg(not(windows))]
    {
        let _ = window;
        let _ = request;
        return Err("工具数据迁移仅支持 Windows 平台".to_string());
    }

    #[cfg(windows)]
    {
        migrate_tool_data_windows(&window, request)
    }
}

#[cfg(windows)]
fn migrate_installed_program_windows(
    request: ProgramMigrationRequest,
) -> Result<ProgramMigrationResult, String> {
    let app_name = sanitize_app_name(&request.app_name)?;
    if app_name.is_empty() {
        return Err("应用名称不能为空".to_string());
    }

    let source = normalize_existing_dir(&request.source_dir, "源目录")?;
    let target = normalize_target_dir(&request.target_dir)?;
    if !source.exists() {
        return Err("源目录不存在".to_string());
    }
    if source == target {
        return Err("源目录和目标目录不能相同".to_string());
    }
    if path_is_nested(&source, &target) || path_is_nested(&target, &source) {
        return Err("源目录和目标目录不能互相包含".to_string());
    }

    let additional_registry_keys = request
        .additional_registry_keys
        .iter()
        .map(|value| validate_registry_path(value))
        .collect::<Result<Vec<_>, _>>()?;

    if request.include_machine_registry && !is_process_elevated_windows()? {
        return Err("需要管理员权限才能更新 HKLM，请点击“提权并重启”后重试。".to_string());
    }
    let additional_shortcut_dirs = request
        .additional_shortcut_dirs
        .iter()
        .map(|value| validate_optional_dir_path(value))
        .collect::<Result<Vec<_>, _>>()?;
    let env_var_names = request
        .env_var_names
        .iter()
        .map(|value| validate_env_var_name(value))
        .collect::<Result<Vec<_>, _>>()?;

    let source_str = source.display().to_string();
    let target_str = target.display().to_string();
    let mut warnings = Vec::new();

    let mut moved = false;
    if !request.dry_run {
        if target.exists() {
            if !target.is_dir() {
                return Err("目标路径已存在且不是目录，请更换目标路径".to_string());
            }
            if !is_directory_empty(&target)? {
                return Err("目标目录已存在且非空，请先清理后重试".to_string());
            }
            copy_dir_recursive(&source, &target)?;
            if !try_remove_source_dir(&source, &mut warnings)? {
                warnings.push("文件已复制到目标目录，但原目录未能自动删除，通常是文件占用导致。请关闭相关程序后手动删除原目录。".to_string());
            }
            moved = true;
        } else {
            move_directory_with_fallback(&source, &target, &mut warnings)?;
            moved = true;
        }
    }

    let registry_updates = update_windows_registry(
        &app_name,
        &source_str,
        &target_str,
        &additional_registry_keys,
        request.include_machine_registry,
        request.dry_run,
        &mut warnings,
    )?;

    let shortcut_updates = update_windows_shortcuts(
        &source_str,
        &target_str,
        &additional_shortcut_dirs,
        request.dry_run,
        &mut warnings,
    )?;

    let env_var_updates = update_windows_environment_vars(
        &source_str,
        &target_str,
        &env_var_names,
        request.include_machine_registry,
        request.dry_run,
        &mut warnings,
    );

    Ok(ProgramMigrationResult {
        app_name,
        source_dir: source_str,
        target_dir: target_str,
        moved,
        registry_updates,
        shortcut_updates,
        env_var_updates,
        warnings,
    })
}

#[cfg(windows)]
fn detect_program_references_windows(
    request: ProgramDetectRequest,
) -> Result<ProgramDetectResult, String> {
    let source = normalize_existing_dir(&request.source_dir, "源目录")?;
    let source_str = source.display().to_string();
    let mut warnings = Vec::new();

    let app_name = request.app_name.trim().to_string();
    let additional_shortcut_dirs = request
        .additional_shortcut_dirs
        .iter()
        .map(|value| validate_optional_dir_path(value))
        .collect::<Result<Vec<_>, _>>()?;
    let env_var_names = request
        .env_var_names
        .iter()
        .map(|value| validate_env_var_name(value))
        .collect::<Result<Vec<_>, _>>()?;

    let registry_keys = detect_registry_related_keys(
        &app_name,
        &source_str,
        request.include_machine_registry,
        &mut warnings,
    )?;
    let shortcut_files =
        detect_shortcut_files(&source_str, &additional_shortcut_dirs, &mut warnings);
    let env_var_matches = detect_environment_var_matches(
        &source_str,
        &env_var_names,
        request.include_machine_registry,
        &mut warnings,
    );

    Ok(ProgramDetectResult {
        normalized_source_dir: source_str,
        registry_keys,
        shortcut_files,
        env_var_matches,
        warnings,
    })
}

#[cfg(windows)]
fn migrate_tool_data_windows(
    window: &tauri::Window,
    request: ToolDataMigrationRequest,
) -> Result<ToolDataMigrationResult, String> {
    emit_tool_migration_log(window, "info", "开始执行工具数据迁移");

    let tool_name = sanitize_app_name(&request.tool_name)?;

    let source_input = expand_windows_env_tokens(request.source_dir.trim());
    let target_input = expand_windows_env_tokens(request.target_dir.trim());

    let source = normalize_existing_dir(&source_input, "源目录")?;
    let target = normalize_target_dir(&target_input)?;
    if source == target {
        return Err("源目录和目标目录不能相同".to_string());
    }
    if path_is_nested(&source, &target) || path_is_nested(&target, &source) {
        return Err("源目录和目标目录不能互相包含".to_string());
    }

    emit_tool_migration_log(
        window,
        "info",
        &format!("准备迁移目录: {} -> {}", source.display(), target.display()),
    );

    let source_str = source.display().to_string();
    let target_str = target.display().to_string();
    let mut warnings = Vec::new();

    let mut moved = false;
    if !request.dry_run {
        emit_tool_migration_log(window, "info", "正在迁移目录数据");
        if target.exists() {
            if !target.is_dir() {
                return Err("目标路径已存在且不是目录".to_string());
            }
            if !is_directory_empty(&target)? {
                return Err("目标目录已存在且非空，请更换目标目录".to_string());
            }
            copy_dir_recursive(&source, &target)?;
            if !try_remove_source_dir(&source, &mut warnings)? {
                warnings.push("源目录未能自动删除，请手动清理。".to_string());
            }
            moved = true;
        } else {
            move_directory_with_fallback(&source, &target, &mut warnings)?;
            moved = true;
        }
        emit_tool_migration_log(window, "success", "目录迁移完成");
    } else if request.dry_run {
        emit_tool_migration_log(window, "info", "dry-run 模式：跳过实际移动");
    }

    let mut symlink_created = false;
    emit_tool_migration_log(window, "info", "正在处理软链接步骤");
    if !request.dry_run {
        symlink_created = create_directory_symlink(&source, &target, &mut warnings)?;
    }
    emit_tool_migration_log(
        window,
        "info",
        if symlink_created {
            "软链接已创建"
        } else if request.dry_run {
            "dry-run 模式：跳过软链接创建"
        } else {
            "软链接未创建（可能已存在或权限不足）"
        },
    );

    emit_tool_migration_log(window, "success", "工具数据迁移任务完成");

    Ok(ToolDataMigrationResult {
        tool_name,
        source_dir: source_str,
        target_dir: target_str,
        moved,
        symlink_created,
        warnings,
    })
}

#[cfg(windows)]
fn emit_tool_migration_log(window: &tauri::Window, level: &str, message: &str) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());

    let payload = ToolMigrationLogEvent {
        level: level.to_string(),
        message: message.to_string(),
        timestamp,
    };

    let _ = window.emit("tool-data-migrate-log", payload);
}

// ── Disk heatmap scan implementation ──

fn emit_disk_scan_progress(window: &tauri::Window, current_path: &str, items_scanned: u64) {
    let payload = DiskScanProgressEvent {
        current_path: current_path.to_string(),
        items_scanned,
    };
    let _ = window.emit("disk-scan-progress", payload);
}

fn metadata_modified_secs(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn modified_secs_of(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|m| metadata_modified_secs(&m))
        .unwrap_or(0)
}

fn scan_directory_recursive(
    path: &Path,
    depth: usize,
    max_depth: Option<usize>,
    include_hidden: bool,
    window: &tauri::Window,
    items_scanned: &AtomicU64,
) -> Result<DiskNode, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let self_path = path.display().to_string();
    let self_modified = modified_secs_of(path);

    let can_recurse = max_depth.map_or(true, |max| depth < max);

    if !can_recurse {
        return Ok(DiskNode {
            name,
            path: self_path,
            size: 0,
            node_type: "folder".to_string(),
            modified_secs: self_modified,
            children: vec![],
        });
    }

    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => {
            return Ok(DiskNode {
                name,
                path: self_path,
                size: 0,
                node_type: "folder".to_string(),
                modified_secs: self_modified,
                children: vec![],
            });
        }
    };

    // Partition immediate entries into subdirectories (recursed in parallel) and
    // files (cheap metadata reads handled inline).
    let mut sub_dirs: Vec<PathBuf> = Vec::new();
    let mut file_children: Vec<DiskNode> = Vec::new();

    for entry in entries.filter_map(Result::ok) {
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        // Skip symlinks to avoid cycles
        if file_type.is_symlink() {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/dirs if not included
        if !include_hidden && file_name.starts_with('.') {
            continue;
        }

        if file_type.is_dir() {
            sub_dirs.push(entry.path());
        } else if file_type.is_file() {
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let file_size = metadata.len();
            let modified = metadata_modified_secs(&metadata);
            let ext = entry
                .path()
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            file_children.push(DiskNode {
                name: file_name,
                path: entry.path().display().to_string(),
                size: file_size,
                node_type: if ext.is_empty() {
                    "other".to_string()
                } else {
                    ext
                },
                modified_secs: modified,
                children: vec![],
            });

            let count = items_scanned.fetch_add(1, Ordering::Relaxed) + 1;
            if count % 500 == 0 {
                emit_disk_scan_progress(window, &entry.path().display().to_string(), count);
            }
        }
    }

    // Recurse into subdirectories in parallel.
    let mut dir_children: Vec<DiskNode> = sub_dirs
        .into_par_iter()
        .filter_map(|child_path| {
            let count = items_scanned.fetch_add(1, Ordering::Relaxed) + 1;
            if count % 500 == 0 {
                emit_disk_scan_progress(window, &child_path.display().to_string(), count);
            }
            scan_directory_recursive(
                &child_path,
                depth + 1,
                max_depth,
                include_hidden,
                window,
                items_scanned,
            )
            .ok()
        })
        .collect();

    let total_size: u64 = dir_children.iter().map(|c| c.size).sum::<u64>()
        + file_children.iter().map(|c| c.size).sum::<u64>();

    // Sort children by size descending (largest first)
    dir_children.sort_by(|a, b| b.size.cmp(&a.size));
    file_children.sort_by(|a, b| b.size.cmp(&a.size));

    let mut children = dir_children;
    children.extend(file_children);

    Ok(DiskNode {
        name,
        path: self_path,
        size: total_size,
        node_type: "folder".to_string(),
        modified_secs: self_modified,
        children,
    })
}

fn count_tree_items(node: &DiskNode) -> (u64, u64) {
    let mut files: u64 = 0;
    let mut dirs: u64 = 0;
    for child in &node.children {
        if child.node_type == "folder" {
            dirs += 1;
            let (f, d) = count_tree_items(child);
            files += f;
            dirs += d;
        } else {
            files += 1;
        }
    }
    (files, dirs)
}

#[tauri::command]
async fn scan_disk_usage(
    window: tauri::Window,
    root_path: String,
    max_depth: Option<usize>,
    include_hidden: Option<bool>,
) -> Result<DiskScanResult, String> {
    let root = PathBuf::from(root_path.trim());
    if root.as_os_str().is_empty() {
        return Err("路径不能为空".to_string());
    }
    if !root.is_absolute() {
        return Err("路径必须是绝对路径".to_string());
    }
    if !root.is_dir() {
        return Err("路径必须是目录".to_string());
    }

    let max_depth = max_depth.filter(|&d| d > 0);
    let include_hidden = include_hidden.unwrap_or(false);

    // Run the heavy recursive scan on a blocking thread so it never stalls the
    // main thread / WebView event loop. A synchronous Tauri command runs on the
    // main thread, so scanning a large drive (e.g. C:) froze the entire UI until
    // it finished. `async` + `spawn_blocking` keeps the window responsive while
    // progress events stream in.
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let items_scanned = AtomicU64::new(0);

        emit_disk_scan_progress(&window, &root.display().to_string(), 0);

        let root_node =
            scan_directory_recursive(&root, 0, max_depth, include_hidden, &window, &items_scanned)?;

        let elapsed = start.elapsed();
        let (total_files, total_dirs) = count_tree_items(&root_node);

        Ok(DiskScanResult {
            total_size: root_node.size,
            total_files,
            total_dirs,
            scan_duration_ms: elapsed.as_millis() as u64,
            root: root_node,
        })
    })
    .await
    .map_err(|error| format!("扫描任务执行失败: {error}"))?
}

// ── Disk heatmap file actions ──

fn validate_action_path(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("路径不能为空".to_string());
    }
    let buf = PathBuf::from(trimmed);
    if !buf.is_absolute() {
        return Err("路径必须是绝对路径".to_string());
    }
    if !buf.exists() {
        return Err("路径不存在".to_string());
    }
    Ok(buf)
}

#[tauri::command]
fn open_path(path: String) -> Result<FileActionResult, String> {
    let target = validate_action_path(&path)?;
    let target_str = target.display().to_string();

    #[cfg(windows)]
    {
        let result = unsafe {
            ShellExecuteW(
                0 as HWND,
                to_wide("open").as_ptr(),
                to_wide(&target_str).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if (result as isize) <= 32 {
            return Err(format!("打开失败: {target_str}"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&target_str)
            .spawn()
            .map_err(|error| format!("打开失败: {error}"))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&target_str)
            .spawn()
            .map_err(|error| format!("打开失败: {error}"))?;
    }

    Ok(FileActionResult {
        success: true,
        message: format!("已打开 {target_str}"),
    })
}

#[tauri::command]
fn reveal_in_file_manager(path: String) -> Result<FileActionResult, String> {
    let target = validate_action_path(&path)?;
    let target_str = target.display().to_string();

    #[cfg(windows)]
    {
        // explorer /select,<path> opens the folder with the item highlighted.
        std::process::Command::new("explorer")
            .arg(format!("/select,{target_str}"))
            .spawn()
            .map_err(|error| format!("在文件管理器中显示失败: {error}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &target_str])
            .spawn()
            .map_err(|error| format!("在文件管理器中显示失败: {error}"))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // No portable "select" on Linux; open the containing directory.
        let parent = target
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| target_str.clone());
        std::process::Command::new("xdg-open")
            .arg(&parent)
            .spawn()
            .map_err(|error| format!("在文件管理器中显示失败: {error}"))?;
    }

    Ok(FileActionResult {
        success: true,
        message: format!("已在文件管理器中显示 {target_str}"),
    })
}

#[tauri::command]
fn delete_path(path: String) -> Result<FileActionResult, String> {
    let target = validate_action_path(&path)?;

    // Refuse to delete a filesystem/drive root (no parent component).
    if target.parent().is_none() {
        return Err("禁止删除驱动器根目录".to_string());
    }

    trash::delete(&target).map_err(|error| format!("删除失败: {error}"))?;

    Ok(FileActionResult {
        success: true,
        message: format!("已移至回收站 {}", target.display()),
    })
}

#[cfg(windows)]
fn is_directory_empty(path: &Path) -> Result<bool, String> {
    let mut entries = fs::read_dir(path).map_err(|error| format!("读取目标目录失败: {error}"))?;
    Ok(entries.next().is_none())
}

#[cfg(windows)]
fn sanitize_app_name(value: &str) -> Result<String, String> {
    let app_name = value.trim();
    if app_name.is_empty() {
        return Err("应用名称不能为空".to_string());
    }
    if app_name.len() > 120 {
        return Err("应用名称过长，请控制在 120 字符以内".to_string());
    }

    let valid = app_name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || " _-().".contains(ch));
    if !valid {
        return Err("应用名称包含非法字符，仅允许字母数字和 _-(). 空格".to_string());
    }

    Ok(app_name.to_string())
}

#[cfg(windows)]
fn normalize_existing_dir(value: &str, label: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label}不能为空"));
    }

    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(format!("{label}必须是绝对路径"));
    }
    let canonical = fs::canonicalize(&path).map_err(|error| format!("读取{label}失败: {error}"))?;
    if !canonical.is_dir() {
        return Err(format!("{label}必须是目录"));
    }
    Ok(canonical)
}

#[cfg(windows)]
fn normalize_target_dir(value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("目标目录不能为空".to_string());
    }

    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err("目标目录必须是绝对路径".to_string());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("目标目录不允许包含 ..".to_string());
    }
    Ok(path)
}

#[cfg(windows)]
fn path_is_nested(parent: &Path, child: &Path) -> bool {
    child.starts_with(parent)
}

#[cfg(windows)]
fn validate_registry_path(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }

    let upper = trimmed.to_ascii_uppercase();
    let allowed_prefix = upper.starts_with("HKLM\\")
        || upper.starts_with("HKEY_LOCAL_MACHINE\\")
        || upper.starts_with("HKCU\\")
        || upper.starts_with("HKEY_CURRENT_USER\\");
    if !allowed_prefix {
        return Err(format!("非法注册表路径: {trimmed}，仅支持 HKLM/HKCU"));
    }

    let has_illegal = trimmed
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '"' | '\'' | ';' | '|'));
    if has_illegal {
        return Err(format!("注册表路径包含非法字符: {trimmed}"));
    }

    Ok(trimmed.to_string())
}

#[cfg(windows)]
fn validate_optional_dir_path(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(format!("附加快捷方式目录必须是绝对路径: {trimmed}"));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("附加快捷方式目录包含非法段 .. : {trimmed}"));
    }
    Ok(trimmed.to_string())
}

#[cfg(windows)]
fn validate_env_var_name(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let valid = trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    if !valid {
        return Err(format!("环境变量名非法: {trimmed}，仅支持字母数字和下划线"));
    }
    Ok(trimmed.to_string())
}

#[cfg(windows)]
fn move_directory_with_fallback(
    source: &Path,
    target: &Path,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| format!("创建目标目录失败: {error}"))?;
        }
    }

    match fs::rename(source, target) {
        Ok(_) => Ok(()),
        Err(_) => {
            copy_dir_recursive(source, target)?;
            if !try_remove_source_dir(source, warnings)? {
                warnings.push("文件已复制到目标目录，但原目录未能自动删除，通常是权限或文件锁导致。请手动删除原目录。".to_string());
            }
            Ok(())
        }
    }
}

#[cfg(windows)]
fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|error| format!("创建目录失败: {error}"))?;

    for entry in fs::read_dir(source).map_err(|error| format!("读取目录失败: {error}"))? {
        let entry = entry.map_err(|error| format!("读取目录项失败: {error}"))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());

        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|error| format!("创建目录失败: {error}"))?;
            }
            fs::copy(&source_path, &target_path)
                .map_err(|error| format!("复制文件失败 ({}): {error}", source_path.display()))?;
        }
    }

    Ok(())
}

#[cfg(windows)]
fn try_remove_source_dir(source: &Path, warnings: &mut Vec<String>) -> Result<bool, String> {
    match fs::remove_dir_all(source) {
        Ok(_) => Ok(true),
        Err(error) => {
            if error.kind() != io::ErrorKind::PermissionDenied {
                return Err(format!("删除原目录失败: {error}"));
            }

            if let Err(attr_error) = clear_readonly_attributes(source) {
                warnings.push(format!("清理只读属性失败: {attr_error}"));
            }

            match fs::remove_dir_all(source) {
                Ok(_) => Ok(true),
                Err(retry_error) => {
                    warnings.push(format!("删除原目录失败: {retry_error}"));
                    Ok(false)
                }
            }
        }
    }
}

#[cfg(windows)]
fn clear_readonly_attributes(root: &Path) -> Result<(), String> {
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        let metadata = fs::metadata(path).map_err(|error| format!("读取文件属性失败: {error}"))?;
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)
                .map_err(|error| format!("设置文件属性失败: {error}"))?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn expand_windows_env_tokens(value: &str) -> String {
    let mut result = String::new();
    let chars = value.chars().collect::<Vec<_>>();
    let mut idx = 0usize;

    while idx < chars.len() {
        if chars[idx] != '%' {
            result.push(chars[idx]);
            idx += 1;
            continue;
        }

        let start = idx + 1;
        let mut end = start;
        while end < chars.len() && chars[end] != '%' {
            end += 1;
        }

        if end >= chars.len() {
            result.push(chars[idx]);
            idx += 1;
            continue;
        }

        let key = chars[start..end].iter().collect::<String>();
        let replacement = env::var(&key).unwrap_or_else(|_| format!("%{key}%"));
        result.push_str(&replacement);
        idx = end + 1;
    }

    result
}

#[cfg(windows)]
fn create_directory_symlink(
    source_path: &Path,
    target_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<bool, String> {
    if source_path.exists() {
        return Ok(false);
    }

    if let Some(parent) = source_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建软链接父目录失败: {error}"))?;
    }

    match std::os::windows::fs::symlink_dir(target_path, source_path) {
        Ok(_) => Ok(true),
        Err(error) => {
            warnings.push(format!("创建目录软链接失败: {error}"));
            Ok(false)
        }
    }
}

#[cfg(windows)]
fn update_windows_registry(
    app_name: &str,
    source_dir: &str,
    target_dir: &str,
    additional_registry_keys: &[String],
    include_machine_registry: bool,
    dry_run: bool,
    warnings: &mut Vec<String>,
) -> Result<usize, String> {
    let mut updates = 0usize;

    let mut roots: Vec<(RegKey, &str)> = Vec::new();

    match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        KEY_READ,
    ) {
        Ok(key) => roots.push((
            key,
            "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        )),
        Err(error) => warnings.push(format!("读取用户卸载注册表失败: {error}")),
    }

    if include_machine_registry {
        match RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            KEY_READ,
        ) {
            Ok(key) => roots.push((
                key,
                "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            )),
            Err(error) => warnings.push(format!("读取机器卸载注册表失败: {error}")),
        }
    }

    for (root, _) in &mut roots {
        for subkey_name in root.enum_keys().flatten() {
            let read_subkey = match root.open_subkey_with_flags(&subkey_name, KEY_READ) {
                Ok(value) => value,
                Err(_) => continue,
            };

            let display_name: String = read_subkey.get_value("DisplayName").unwrap_or_default();
            let install_location: String =
                read_subkey.get_value("InstallLocation").unwrap_or_default();
            let display_icon: String = read_subkey.get_value("DisplayIcon").unwrap_or_default();

            let matched = display_name
                .to_ascii_lowercase()
                .contains(&app_name.to_ascii_lowercase())
                || string_contains_path_ci(&install_location, source_dir)
                || string_contains_path_ci(&display_icon, source_dir)
                || key_contains_source_string_values(&read_subkey, source_dir);

            if !matched {
                continue;
            }

            let writable_subkey =
                match root.open_subkey_with_flags(&subkey_name, KEY_READ | KEY_WRITE) {
                    Ok(value) => value,
                    Err(error) => {
                        warnings.push(format!("注册表写入权限不足 ({subkey_name}): {error}"));
                        continue;
                    }
                };

            updates +=
                rewrite_string_values_in_key(&writable_subkey, source_dir, target_dir, dry_run)?;
        }
    }

    for key_path in additional_registry_keys {
        let trimmed = key_path.trim();
        if trimmed.is_empty() {
            continue;
        }
        match open_registry_key_for_write(trimmed) {
            Ok(key) => {
                updates += rewrite_string_values_in_key(&key, source_dir, target_dir, dry_run)?;
            }
            Err(error) => warnings.push(format!("自定义注册表路径处理失败 ({trimmed}): {error}")),
        }
    }

    Ok(updates)
}

#[cfg(windows)]
fn detect_registry_related_keys(
    app_name: &str,
    source_dir: &str,
    include_machine_registry: bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    let mut roots: Vec<(RegKey, &str)> = Vec::new();
    let mut results = Vec::new();

    match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        KEY_READ,
    ) {
        Ok(key) => roots.push((
            key,
            "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        )),
        Err(error) => warnings.push(format!("读取用户卸载注册表失败: {error}")),
    }

    if include_machine_registry {
        match RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            KEY_READ,
        ) {
            Ok(key) => roots.push((
                key,
                "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            )),
            Err(error) => warnings.push(format!("读取机器卸载注册表失败: {error}")),
        }
    }

    for (root, root_path) in &mut roots {
        for subkey_name in root.enum_keys().flatten() {
            let read_subkey = match root.open_subkey_with_flags(&subkey_name, KEY_READ) {
                Ok(value) => value,
                Err(_) => continue,
            };

            let display_name: String = read_subkey.get_value("DisplayName").unwrap_or_default();
            let install_location: String =
                read_subkey.get_value("InstallLocation").unwrap_or_default();
            let display_icon: String = read_subkey.get_value("DisplayIcon").unwrap_or_default();

            let app_match = if app_name.trim().is_empty() {
                false
            } else {
                display_name
                    .to_ascii_lowercase()
                    .contains(&app_name.to_ascii_lowercase())
            };

            let matched = app_match
                || string_contains_path_ci(&install_location, source_dir)
                || string_contains_path_ci(&display_icon, source_dir)
                || key_contains_source_string_values(&read_subkey, source_dir);

            if matched {
                results.push(format!("{root_path}\\{subkey_name}"));
            }
        }
    }

    results.sort();
    results.dedup();
    Ok(results)
}

#[cfg(windows)]
fn key_contains_source_string_values(key: &RegKey, source_dir: &str) -> bool {
    key.enum_values().flatten().any(|(name, _)| {
        let value: Result<String, _> = key.get_value(&name);
        matches!(value, Ok(content) if string_contains_path_ci(&content, source_dir))
    })
}

#[cfg(windows)]
fn open_registry_key_for_write(path: &str) -> Result<RegKey, String> {
    let upper = path.to_ascii_uppercase();
    if let Some(rest) = upper.strip_prefix("HKLM\\") {
        return RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(rest, KEY_READ | KEY_WRITE)
            .map_err(|error| error.to_string());
    }
    if let Some(rest) = upper.strip_prefix("HKEY_LOCAL_MACHINE\\") {
        return RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(rest, KEY_READ | KEY_WRITE)
            .map_err(|error| error.to_string());
    }
    if let Some(rest) = upper.strip_prefix("HKCU\\") {
        return RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(rest, KEY_READ | KEY_WRITE)
            .map_err(|error| error.to_string());
    }
    if let Some(rest) = upper.strip_prefix("HKEY_CURRENT_USER\\") {
        return RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(rest, KEY_READ | KEY_WRITE)
            .map_err(|error| error.to_string());
    }

    Err("仅支持 HKLM/HKCU 路径".to_string())
}

#[cfg(windows)]
fn rewrite_string_values_in_key(
    key: &RegKey,
    source_dir: &str,
    target_dir: &str,
    dry_run: bool,
) -> Result<usize, String> {
    let mut updates = 0usize;
    for value_name in key.enum_values().flatten().map(|(name, _)| name) {
        let value: Result<String, _> = key.get_value(&value_name);
        let Ok(current) = value else { continue };

        if !string_contains_path_ci(&current, source_dir) {
            continue;
        }

        updates += 1;
        if dry_run {
            continue;
        }

        let replaced = replace_path_ci(&current, source_dir, target_dir);
        key.set_value(&value_name, &replaced)
            .map_err(|error| format!("更新注册表值失败 ({value_name}): {error}"))?;
    }

    Ok(updates)
}

#[cfg(windows)]
fn update_windows_shortcuts(
    source_dir: &str,
    target_dir: &str,
    additional_shortcut_dirs: &[String],
    dry_run: bool,
    warnings: &mut Vec<String>,
) -> Result<usize, String> {
    let affected_files = detect_shortcut_files(source_dir, additional_shortcut_dirs, warnings);
    let affected = affected_files.len();

    if affected > 0 && !dry_run {
        warnings.push(format!(
            "检测到 {affected} 个快捷方式引用旧路径。为降低安全风险，已禁用 PowerShell 自动改写，请手动重建或修改快捷方式到: {target_dir}"
        ));
    }

    Ok(affected)
}

#[cfg(windows)]
fn detect_shortcut_files(
    source_dir: &str,
    additional_shortcut_dirs: &[String],
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut search_dirs: Vec<String> = vec![
        env::var("APPDATA")
            .map(|value| format!("{value}\\Microsoft\\Windows\\Start Menu"))
            .unwrap_or_default(),
        env::var("PROGRAMDATA")
            .map(|value| format!("{value}\\Microsoft\\Windows\\Start Menu"))
            .unwrap_or_default(),
        env::var("USERPROFILE")
            .map(|value| format!("{value}\\Desktop"))
            .unwrap_or_default(),
        env::var("PUBLIC")
            .map(|value| format!("{value}\\Desktop"))
            .unwrap_or_default(),
    ];

    search_dirs.extend(
        additional_shortcut_dirs
            .iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty()),
    );

    search_dirs.retain(|dir| !dir.is_empty());
    search_dirs.sort();
    search_dirs.dedup();

    let mut files = Vec::new();
    for dir in &search_dirs {
        let dir_path = PathBuf::from(dir);
        if !dir_path.exists() {
            continue;
        }

        for entry in WalkDir::new(&dir_path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
        {
            let path = entry.path();
            let ext = path
                .extension()
                .map(|value| value.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if ext != "lnk" {
                continue;
            }

            let bytes = match fs::read(path) {
                Ok(value) => value,
                Err(error) => {
                    warnings.push(format!("读取快捷方式失败 ({}): {error}", path.display()));
                    continue;
                }
            };

            if shortcut_bytes_reference_source(&bytes, source_dir) {
                files.push(path.display().to_string());
            }
        }
    }

    files.sort();
    files.dedup();
    files
}

#[cfg(windows)]
fn shortcut_bytes_reference_source(bytes: &[u8], source_dir: &str) -> bool {
    if source_dir.is_empty() {
        return false;
    }

    let source_utf8 = source_dir.as_bytes();
    if bytes
        .windows(source_utf8.len())
        .any(|window| window == source_utf8)
    {
        return true;
    }

    let source_utf16le = source_dir
        .encode_utf16()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<u8>>();
    if source_utf16le.is_empty() {
        return false;
    }

    bytes
        .windows(source_utf16le.len())
        .any(|window| window == source_utf16le)
}

#[cfg(windows)]
fn update_windows_environment_vars(
    source_dir: &str,
    target_dir: &str,
    env_var_names: &[String],
    include_machine_scope: bool,
    dry_run: bool,
    warnings: &mut Vec<String>,
) -> usize {
    let mut updates = 0usize;
    let mut names = env_var_names
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();

    if names.is_empty() {
        names.push("Path".to_string());
    }

    let user_env = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .ok();
    let machine_env = if include_machine_scope {
        RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(
                "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                KEY_READ | KEY_WRITE,
            )
            .ok()
    } else {
        None
    };

    for name in names {
        if let Some(key) = &user_env {
            updates += rewrite_env_var_value(key, &name, source_dir, target_dir, dry_run, warnings);
        }
        if let Some(key) = &machine_env {
            updates += rewrite_env_var_value(key, &name, source_dir, target_dir, dry_run, warnings);
        }
    }

    updates
}

#[cfg(windows)]
fn detect_environment_var_matches(
    source_dir: &str,
    env_var_names: &[String],
    include_machine_scope: bool,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut names = env_var_names
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    if names.is_empty() {
        names.push("Path".to_string());
    }

    let user_env = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ)
        .ok();
    let machine_env = if include_machine_scope {
        RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(
                "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
                KEY_READ,
            )
            .ok()
    } else {
        None
    };

    let mut matches = Vec::new();
    for name in names {
        if let Some(key) = &user_env {
            let value: Result<String, _> = key.get_value(&name);
            if let Ok(content) = value {
                if string_contains_path_ci(&content, source_dir) {
                    matches.push(format!("HKCU\\Environment\\{name}"));
                }
            }
        }

        if let Some(key) = &machine_env {
            let value: Result<String, _> = key.get_value(&name);
            if let Ok(content) = value {
                if string_contains_path_ci(&content, source_dir) {
                    matches.push(format!(
                        "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment\\{name}"
                    ));
                }
            } else {
                warnings.push(format!("读取机器环境变量失败: {name}"));
            }
        }
    }

    matches.sort();
    matches.dedup();
    matches
}

#[cfg(windows)]
fn rewrite_env_var_value(
    key: &RegKey,
    name: &str,
    source_dir: &str,
    target_dir: &str,
    dry_run: bool,
    warnings: &mut Vec<String>,
) -> usize {
    let current: Result<String, _> = key.get_value(name);
    let Ok(current) = current else { return 0 };

    if !string_contains_path_ci(&current, source_dir) {
        return 0;
    }

    if dry_run {
        return 1;
    }

    let replaced = replace_path_ci(&current, source_dir, target_dir);
    match key.set_raw_value(
        name,
        &RegValue {
            bytes: replaced
                .encode_utf16()
                .flat_map(|value| value.to_le_bytes())
                .chain([0, 0])
                .collect::<Vec<u8>>(),
            vtype: winreg::enums::RegType::REG_EXPAND_SZ,
        },
    ) {
        Ok(_) => 1,
        Err(error) => {
            warnings.push(format!("更新环境变量失败 ({name}): {error}"));
            0
        }
    }
}

#[cfg(windows)]
fn string_contains_path_ci(text: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    text.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

#[cfg(windows)]
fn replace_path_ci(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return text.to_string();
    }

    let text_lower = text.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();

    let mut result = String::with_capacity(text.len());
    let mut cursor = 0usize;

    while let Some(relative_pos) = text_lower[cursor..].find(&needle_lower) {
        let start = cursor + relative_pos;
        result.push_str(&text[cursor..start]);
        result.push_str(replacement);
        cursor = start + needle.len();
    }

    result.push_str(&text[cursor..]);
    result
}

#[cfg(windows)]
fn is_process_elevated_windows() -> Result<bool, String> {
    let mut token: HANDLE = std::ptr::null_mut();
    let process = unsafe { GetCurrentProcess() };

    let opened = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token as *mut HANDLE) };
    if opened == 0 {
        return Err("无法读取当前进程令牌".to_string());
    }

    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut return_length: u32 = 0;
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_length as *mut u32,
        )
    };

    unsafe {
        CloseHandle(token);
    }

    if result == 0 {
        return Err("无法判断是否为管理员权限".to_string());
    }

    Ok(elevation.TokenIsElevated != 0)
}

#[cfg(windows)]
fn relaunch_as_admin_windows() -> Result<(), String> {
    if is_process_elevated_windows()? {
        return Ok(());
    }

    let exe_path = env::current_exe().map_err(|error| format!("读取当前程序路径失败: {error}"))?;
    let exe = exe_path.to_string_lossy().to_string();

    let verb = to_wide("runas");
    let file = to_wide(&exe);

    let result = unsafe {
        ShellExecuteW(
            0 as HWND,
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    if (result as isize) <= 32 {
        return Err("提权启动被取消或失败，请以管理员身份运行后重试。".to_string());
    }

    std::process::exit(0);
}

#[cfg(windows)]
fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn build_exclude_set(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for raw in patterns {
        let pattern = raw.trim();
        if pattern.is_empty() {
            continue;
        }
        let glob =
            Glob::new(pattern).map_err(|error| format!("过滤条件无效 ({pattern}): {error}"))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|error| format!("构建过滤条件失败: {error}"))
}

fn normalize_rel_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn collect_files_for_archive(
    source: &Path,
    exclude_set: &GlobSet,
) -> Result<Vec<(PathBuf, String)>, String> {
    let mut files = Vec::new();

    if source.is_file() {
        let name = source
            .file_name()
            .ok_or_else(|| "无法识别源文件名".to_string())?
            .to_string_lossy()
            .to_string();
        if !exclude_set.is_match(&name) {
            files.push((source.to_path_buf(), name));
        }
        return Ok(files);
    }

    let root_name = source
        .file_name()
        .ok_or_else(|| "无法识别源目录名".to_string())?
        .to_string_lossy()
        .to_string();

    let walker = WalkDir::new(source)
        .into_iter()
        .filter_entry(|entry| should_walk(entry.path(), source, exclude_set));

    for entry in walker.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let relative = path
            .strip_prefix(source)
            .map_err(|error| format!("计算相对路径失败: {error}"))?;
        let relative_normalized = normalize_rel_path(relative);
        if exclude_set.is_match(&relative_normalized) {
            continue;
        }

        let archive_name = format!("{root_name}/{relative_normalized}");
        files.push((path.to_path_buf(), archive_name));
    }

    Ok(files)
}

fn should_walk(path: &Path, source: &Path, exclude_set: &GlobSet) -> bool {
    if path == source {
        return true;
    }
    match path.strip_prefix(source) {
        Ok(relative) => {
            let normalized = normalize_rel_path(relative);
            !exclude_set.is_match(&normalized)
        }
        Err(_) => true,
    }
}

fn count_skipped_entries(source: &Path, exclude_set: &GlobSet) -> usize {
    if source.is_file() {
        let file_name = source
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        return usize::from(exclude_set.is_match(file_name));
    }

    WalkDir::new(source)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            if entry.path() == source {
                return false;
            }
            entry
                .path()
                .strip_prefix(source)
                .map(|relative| exclude_set.is_match(normalize_rel_path(relative)))
                .unwrap_or(false)
        })
        .count()
}

fn compress_to_zip(files: &[(PathBuf, String)], output: &Path) -> Result<(), String> {
    let file = File::create(output).map_err(|error| format!("创建 ZIP 失败: {error}"))?;
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);

    for (source, archive_name) in files {
        let mut input = File::open(source).map_err(|error| format!("读取源文件失败: {error}"))?;
        writer
            .start_file(archive_name, options)
            .map_err(|error| format!("写入 ZIP 条目失败: {error}"))?;
        io::copy(&mut input, &mut writer).map_err(|error| format!("写入 ZIP 内容失败: {error}"))?;
    }

    writer
        .finish()
        .map_err(|error| format!("完成 ZIP 压缩失败: {error}"))?;
    Ok(())
}

fn compress_to_tar(files: &[(PathBuf, String)], output: &Path) -> Result<(), String> {
    let file = File::create(output).map_err(|error| format!("创建 TAR 失败: {error}"))?;
    let mut builder = TarBuilder::new(file);
    append_files_to_tar(&mut builder, files)?;
    builder
        .finish()
        .map_err(|error| format!("完成 TAR 压缩失败: {error}"))?;
    Ok(())
}

fn compress_to_targz(files: &[(PathBuf, String)], output: &Path) -> Result<(), String> {
    let file = File::create(output).map_err(|error| format!("创建 tar.gz 失败: {error}"))?;
    let encoder = GzEncoder::new(file, GzCompression::default());
    let mut builder = TarBuilder::new(encoder);
    append_files_to_tar(&mut builder, files)?;
    builder
        .finish()
        .map_err(|error| format!("完成 tar.gz 压缩失败: {error}"))?;
    Ok(())
}

fn compress_to_tarbz2(files: &[(PathBuf, String)], output: &Path) -> Result<(), String> {
    let file = File::create(output).map_err(|error| format!("创建 tar.bz2 失败: {error}"))?;
    let encoder = BzEncoder::new(file, BzCompression::default());
    let mut builder = TarBuilder::new(encoder);
    append_files_to_tar(&mut builder, files)?;
    builder
        .finish()
        .map_err(|error| format!("完成 tar.bz2 压缩失败: {error}"))?;
    Ok(())
}

fn compress_to_tarxz(files: &[(PathBuf, String)], output: &Path) -> Result<(), String> {
    let file = File::create(output).map_err(|error| format!("创建 tar.xz 失败: {error}"))?;
    let encoder = XzEncoder::new(file, 6);
    let mut builder = TarBuilder::new(encoder);
    append_files_to_tar(&mut builder, files)?;
    builder
        .finish()
        .map_err(|error| format!("完成 tar.xz 压缩失败: {error}"))?;
    Ok(())
}

fn append_files_to_tar<W: io::Write>(
    builder: &mut TarBuilder<W>,
    files: &[(PathBuf, String)],
) -> Result<(), String> {
    for (source, archive_name) in files {
        builder
            .append_path_with_name(source, archive_name)
            .map_err(|error| format!("写入 TAR 条目失败: {error}"))?;
    }
    Ok(())
}

fn extract_zip(
    archive_path: &Path,
    output_dir: &Path,
    exclude_set: &GlobSet,
) -> Result<(usize, usize), String> {
    let file = File::open(archive_path).map_err(|error| format!("打开 ZIP 失败: {error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|error| format!("读取 ZIP 失败: {error}"))?;
    let mut processed = 0usize;
    let mut skipped = 0usize;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("读取 ZIP 条目失败: {error}"))?;

        let entry_name = entry.name().replace('\\', "/");
        if exclude_set.is_match(&entry_name) {
            skipped += 1;
            continue;
        }

        let safe_relative = sanitize_relative_path(Path::new(&entry_name))?;
        let destination = output_dir.join(&safe_relative);

        if entry.is_dir() {
            fs::create_dir_all(&destination).map_err(|error| format!("创建目录失败: {error}"))?;
            processed += 1;
            continue;
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建目录失败: {error}"))?;
        }
        let mut output =
            File::create(&destination).map_err(|error| format!("创建文件失败: {error}"))?;
        io::copy(&mut entry, &mut output).map_err(|error| format!("写入文件失败: {error}"))?;
        processed += 1;
    }

    Ok((processed, skipped))
}

fn extract_tar(
    archive_path: &Path,
    output_dir: &Path,
    exclude_set: &GlobSet,
) -> Result<(usize, usize), String> {
    let file = File::open(archive_path).map_err(|error| format!("打开 TAR 失败: {error}"))?;
    let archive = TarArchive::new(file);
    extract_tar_entries(archive, output_dir, exclude_set)
}

fn extract_targz(
    archive_path: &Path,
    output_dir: &Path,
    exclude_set: &GlobSet,
) -> Result<(usize, usize), String> {
    let file = File::open(archive_path).map_err(|error| format!("打开 tar.gz 失败: {error}"))?;
    let decoder = GzDecoder::new(file);
    let archive = TarArchive::new(decoder);
    extract_tar_entries(archive, output_dir, exclude_set)
}

fn extract_tarbz2(
    archive_path: &Path,
    output_dir: &Path,
    exclude_set: &GlobSet,
) -> Result<(usize, usize), String> {
    let file = File::open(archive_path).map_err(|error| format!("打开 tar.bz2 失败: {error}"))?;
    let decoder = BzDecoder::new(file);
    let archive = TarArchive::new(decoder);
    extract_tar_entries(archive, output_dir, exclude_set)
}

fn extract_tarxz(
    archive_path: &Path,
    output_dir: &Path,
    exclude_set: &GlobSet,
) -> Result<(usize, usize), String> {
    let file = File::open(archive_path).map_err(|error| format!("打开 tar.xz 失败: {error}"))?;
    let decoder = XzDecoder::new(file);
    let archive = TarArchive::new(decoder);
    extract_tar_entries(archive, output_dir, exclude_set)
}

fn extract_tar_entries<R: io::Read>(
    mut archive: TarArchive<R>,
    output_dir: &Path,
    exclude_set: &GlobSet,
) -> Result<(usize, usize), String> {
    let mut processed = 0usize;
    let mut skipped = 0usize;

    let entries = archive
        .entries()
        .map_err(|error| format!("读取 TAR 条目失败: {error}"))?;

    for entry_result in entries {
        let mut entry = entry_result.map_err(|error| format!("读取 TAR 条目失败: {error}"))?;
        let path = entry
            .path()
            .map_err(|error| format!("读取 TAR 路径失败: {error}"))?
            .to_path_buf();

        let normalized = normalize_rel_path(&path);
        if exclude_set.is_match(&normalized) {
            skipped += 1;
            continue;
        }

        let safe_relative = sanitize_relative_path(&path)?;
        let destination = output_dir.join(safe_relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建目录失败: {error}"))?;
        }

        entry
            .unpack(destination)
            .map_err(|error| format!("解压 TAR 条目失败: {error}"))?;
        processed += 1;
    }

    Ok((processed, skipped))
}

fn sanitize_relative_path(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        return Err("压缩包包含绝对路径，已拒绝处理".to_string());
    }

    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => safe.push(value),
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err("压缩包包含不安全路径，已拒绝处理".to_string());
            }
        }
    }

    if safe.as_os_str().is_empty() {
        return Err("压缩包条目路径无效".to_string());
    }

    Ok(safe)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_system_info,
            list_directory,
            compress_archive,
            extract_archive,
            is_process_elevated,
            relaunch_as_admin,
            detect_program_references,
            migrate_installed_program,
            migrate_tool_data,
            scan_disk_usage,
            check_directory_locked,
            open_path,
            reveal_in_file_manager,
            delete_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{validate_action_path, ToolDataMigrationRequest, ToolDataMigrationResult};

    // SC-004: a tool-data migration request deserializes from the symlink-only
    // payload — i.e. without any `strategy` or `envVarName` fields.
    #[test]
    fn request_deserializes_without_strategy_or_env_fields() {
        let json = r#"{
            "toolName": "rustup",
            "sourceDir": "C:/Users/me/.rustup",
            "targetDir": "D:/tool-data/.rustup",
            "dryRun": true
        }"#;
        let req: ToolDataMigrationRequest =
            serde_json::from_str(json).expect("request should deserialize");
        assert_eq!(req.tool_name, "rustup");
        assert!(req.dry_run);
    }

    // SC-004: the result serialized to the frontend carries no env-var fields
    // (`envVarUpdated`) and no `strategy` — symlink-only contract.
    #[test]
    fn result_serializes_without_env_fields() {
        let result = ToolDataMigrationResult {
            tool_name: "rustup".to_string(),
            source_dir: "C:/Users/me/.rustup".to_string(),
            target_dir: "D:/tool-data/.rustup".to_string(),
            moved: false,
            symlink_created: false,
            warnings: vec![],
        };
        let value = serde_json::to_value(&result).expect("result should serialize");
        let obj = value.as_object().expect("result is a JSON object");
        assert!(
            !obj.contains_key("envVarUpdated"),
            "envVarUpdated must be removed"
        );
        assert!(!obj.contains_key("strategy"), "strategy must be removed");
        assert!(
            obj.contains_key("symlinkCreated"),
            "symlinkCreated must remain"
        );
    }

    // File-action path validation must reject empty and relative paths before any
    // filesystem mutation (open / reveal / delete share this guard).
    #[test]
    fn validate_action_path_rejects_empty() {
        assert!(validate_action_path("").is_err());
        assert!(validate_action_path("   ").is_err());
    }

    #[test]
    fn validate_action_path_rejects_relative() {
        assert!(validate_action_path("relative/path").is_err());
        assert!(validate_action_path("./foo").is_err());
    }
}
