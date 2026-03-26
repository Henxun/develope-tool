use bzip2::{read::BzDecoder, write::BzEncoder, Compression as BzCompression};
use flate2::{read::GzDecoder, write::GzEncoder, Compression as GzCompression};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Serialize;
use std::{
    env,
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
};
use tar::{Archive as TarArchive, Builder as TarBuilder};
use walkdir::WalkDir;
use xz2::{read::XzDecoder, write::XzEncoder};
use zip::{write::FileOptions, CompressionMethod, ZipArchive, ZipWriter};

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

fn build_exclude_set(patterns: &[String]) -> Result<GlobSet, String> {
    let mut builder = GlobSetBuilder::new();
    for raw in patterns {
        let pattern = raw.trim();
        if pattern.is_empty() {
            continue;
        }
        let glob = Glob::new(pattern).map_err(|error| format!("过滤条件无效 ({pattern}): {error}"))?;
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
        let mut output = File::create(&destination).map_err(|error| format!("创建文件失败: {error}"))?;
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
            extract_archive
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
