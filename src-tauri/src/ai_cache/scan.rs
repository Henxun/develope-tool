//! 体积扫描：只累加字节数，不保留文件树；不跟随符号链接。

use super::catalog::{self, Platform, ResolvedRoot, ToolSpec};
use super::{
    AiCacheCategoryDto, AiCachePathDto, AiCacheRootDto, AiCacheScanProgressEvent,
    AiCacheScanResult, AiPathKind, AiToolScanDto, CATALOG_VERSION,
};
use rayon::prelude::*;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

/// 目录内最近 10 分钟内有修改 → 可能正在使用（默认不勾选）。
const IN_USE_WINDOW_MS: u64 = 10 * 60 * 1000;
/// 进度事件节流：每工具最多约 5 次/秒。
const PROGRESS_INTERVAL_MS: u64 = 200;

type Lookup<'a> = &'a (dyn Fn(&str) -> Option<OsString> + Sync);
type ProgressSink<'a> = &'a (dyn Fn(AiCacheScanProgressEvent) + Sync);

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn modified_millis(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

struct ScanProgress<'a> {
    tool_index: usize,
    tool_count: usize,
    tool_id: &'static str,
    scanned: AtomicU64,
    last_emit_ms: AtomicU64,
    sink: ProgressSink<'a>,
}

impl<'a> ScanProgress<'a> {
    fn maybe_emit(&self, path: &Path, force: bool) {
        let scanned_entries = self.scanned.fetch_add(1, Ordering::Relaxed) + 1;
        let now = epoch_millis();
        let last = self.last_emit_ms.load(Ordering::Relaxed);
        if !force && now.saturating_sub(last) < PROGRESS_INTERVAL_MS {
            return;
        }
        if !force
            && self
                .last_emit_ms
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            return;
        }
        if force {
            self.last_emit_ms.store(now, Ordering::Relaxed);
        }
        (self.sink)(AiCacheScanProgressEvent {
            tool_id: self.tool_id.to_string(),
            current_path: path.display().to_string(),
            scanned_entries,
            tool_index: self.tool_index,
            tool_count: self.tool_count,
        });
    }
}

/// 单个路径的扫描结果。
#[derive(Debug, Clone, Copy, Default)]
pub struct MeasureOutcome {
    pub bytes: u64,
    pub last_modified_ms: u64,
    pub unreadable: u64,
    pub is_dir: bool,
}

/// 无进度回调的体积统计（清理前统计实际释放量时复用）。
pub fn measure_bytes(path: &Path) -> MeasureOutcome {
    measure_with(path, &mut |_| {})
}

fn measure_with(path: &Path, on_entry: &mut dyn FnMut(&Path)) -> MeasureOutcome {
    let mut outcome = MeasureOutcome::default();
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            outcome.unreadable += 1;
            return outcome;
        }
    };
    // 白名单目标本身是符号链接：不跟随、不统计（读取调用方负责给出提示）。
    if metadata.file_type().is_symlink() {
        return outcome;
    }

    on_entry(path);
    outcome.is_dir = metadata.is_dir();

    if metadata.is_file() {
        outcome.bytes = metadata.len();
        outcome.last_modified_ms = modified_millis(&metadata);
        return outcome;
    }
    if !metadata.is_dir() {
        return outcome;
    }

    for entry in WalkDir::new(path).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                outcome.unreadable += 1;
                continue;
            }
        };
        if entry.file_type().is_symlink() {
            continue;
        }
        on_entry(entry.path());
        if entry.file_type().is_file() {
            match entry.metadata() {
                Ok(metadata) => {
                    outcome.bytes = outcome.bytes.saturating_add(metadata.len());
                    let modified = modified_millis(&metadata);
                    if modified > outcome.last_modified_ms {
                        outcome.last_modified_ms = modified;
                    }
                }
                Err(_) => outcome.unreadable += 1,
            }
        }
    }

    outcome
}

fn empty_category(category: super::AiCacheCategory, tool_cleanable: bool) -> AiCacheCategoryDto {
    let meta = catalog::category_meta(category);
    AiCacheCategoryDto {
        category,
        label: meta.label.to_string(),
        risk: meta.risk,
        cleanable: category.is_cleanable() && tool_cleanable,
        // 根目录不存在 = 没有可计算的内容；保持 true，避免前端显示"未计算"。
        measured: true,
        default_selected: meta.default_selected,
        description: meta.description.to_string(),
        paths: Vec::new(),
        bytes: 0,
    }
}

/// 降级工具的类别说明：替代默认的"可安全删除"文案，避免误导。
const DISABLED_DESCRIPTION: &str =
    "该工具根目录指向敏感位置，已降级为仅检测，不提供清理（详见工具提示）。";

fn scan_root(
    root: &ResolvedRoot,
    spec: &ToolSpec,
    progress: &ScanProgress<'_>,
    tool_cleanable: bool,
    include_heavy: bool,
) -> AiCacheRootDto {
    let mut categories = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    if !root.exists {
        for category_spec in spec.categories {
            categories.push(empty_category(category_spec.category, tool_cleanable));
        }
        return AiCacheRootDto {
            path: root.path.display().to_string(),
            exists: false,
            categories,
            warnings,
        };
    }

    let now = epoch_millis();
    for category_spec in spec.categories {
        let meta = catalog::category_meta(category_spec.category);
        let category_cleanable = category_spec.category.is_cleanable() && tool_cleanable;
        // 惰性计算（Chief 裁决 2）：主扫描不递归统计 sessions / model_cache，
        // 只列出路径；带 includeHeavy 的二次请求才算体积。
        let measured = include_heavy || category_cleanable;
        let description = if tool_cleanable {
            meta.description.to_string()
        } else {
            DISABLED_DESCRIPTION.to_string()
        };
        let mut paths: Vec<AiCachePathDto> = Vec::new();
        let mut category_bytes: u64 = 0;

        for target in catalog::expand_targets(&root.path, category_spec) {
            let metadata = match fs::symlink_metadata(&target.path) {
                Ok(metadata) => metadata,
                // 不存在的白名单目标不返回，避免用户勾选"空气"。
                Err(_) => continue,
            };
            if metadata.file_type().is_symlink() {
                warnings.push(format!("已跳过符号链接：{}", target.path.display()));
                continue;
            }

            if !measured {
                progress.maybe_emit(&target.path, false);
                paths.push(AiCachePathDto {
                    path: target.path.display().to_string(),
                    bytes: 0,
                    kind: if metadata.is_dir() {
                        AiPathKind::Dir
                    } else {
                        AiPathKind::File
                    },
                    last_modified_ms: 0,
                    in_use: false,
                });
                continue;
            }

            let outcome = measure_with(&target.path, &mut |entry| {
                progress.maybe_emit(entry, false);
            });
            if outcome.unreadable > 0 {
                warnings.push(format!(
                    "部分内容无法读取（{} 项）：{}",
                    outcome.unreadable,
                    target.path.display()
                ));
            }

            let kind = if outcome.is_dir {
                AiPathKind::Dir
            } else {
                AiPathKind::File
            };
            let in_use = outcome.last_modified_ms > 0
                && now.saturating_sub(outcome.last_modified_ms) <= IN_USE_WINDOW_MS;

            category_bytes = category_bytes.saturating_add(outcome.bytes);
            paths.push(AiCachePathDto {
                path: target.path.display().to_string(),
                bytes: outcome.bytes,
                kind,
                last_modified_ms: outcome.last_modified_ms,
                in_use,
            });
        }

        categories.push(AiCacheCategoryDto {
            category: category_spec.category,
            label: meta.label.to_string(),
            risk: meta.risk,
            cleanable: category_cleanable,
            measured,
            default_selected: meta.default_selected,
            description,
            paths,
            bytes: category_bytes,
        });
    }

    AiCacheRootDto {
        path: root.path.display().to_string(),
        exists: true,
        categories,
        warnings,
    }
}

fn scan_tool(
    spec: &'static ToolSpec,
    platform: Platform,
    lookup: Lookup<'_>,
    progress: &ScanProgress<'_>,
    include_heavy: bool,
) -> AiToolScanDto {
    let roots = catalog::resolve_roots_with(spec, platform, lookup);
    // 降级判定（Chief 裁决 1）：任何根目录指向 home / home 上级 / 文件系统根时，
    // 整个工具只检测不清理。
    let policy =
        catalog::tool_policy_for_roots(&roots, catalog::resolve_home(platform, lookup).as_deref());
    let root_dtos: Vec<AiCacheRootDto> = roots
        .iter()
        .map(|root| scan_root(root, spec, progress, policy.cleanable, include_heavy))
        .collect();

    let detected = root_dtos.iter().any(|root| root.exists);
    let mut cleanable_bytes: u64 = 0;
    let mut display_only_bytes: u64 = 0;
    let mut warnings: Vec<String> = policy.warnings.clone();
    for root in &root_dtos {
        warnings.extend(root.warnings.iter().cloned());
        for category in &root.categories {
            if !category.measured {
                continue;
            }
            if category.cleanable {
                cleanable_bytes = cleanable_bytes.saturating_add(category.bytes);
            } else {
                display_only_bytes = display_only_bytes.saturating_add(category.bytes);
            }
        }
    }

    AiToolScanDto {
        id: spec.id.to_string(),
        name: spec.name.to_string(),
        vendor: spec.vendor.to_string(),
        detected,
        cleanable: policy.cleanable,
        roots: root_dtos,
        cleanable_bytes,
        display_only_bytes,
        warnings,
    }
}

/// 扫描入口（保持旧签名：等价于 `include_heavy = false` 的惰性主扫描）。
pub fn scan_all(
    platform: Platform,
    lookup: Lookup<'_>,
    tool_ids: &[String],
    sink: ProgressSink<'_>,
) -> Result<AiCacheScanResult, String> {
    scan_all_with_heavy(platform, lookup, tool_ids, false, sink)
}

/// 扫描入口。`tool_ids` 为空表示全部工具；未知 id 返回命令级错误（不静默忽略）。
///
/// `include_heavy=false`：只统计可清理类别，展示类别只列路径（AC-05 的 5s 预算）；
/// `include_heavy=true`：连 sessions / model_cache 一起递归统计（前端「计算占用」）。
pub fn scan_all_with_heavy(
    platform: Platform,
    lookup: Lookup<'_>,
    tool_ids: &[String],
    include_heavy: bool,
    sink: ProgressSink<'_>,
) -> Result<AiCacheScanResult, String> {
    let all = catalog::tools();
    let specs: Vec<&'static ToolSpec> = if tool_ids.is_empty() {
        all.iter().collect()
    } else {
        let mut selected: Vec<&'static ToolSpec> = Vec::with_capacity(tool_ids.len());
        for id in tool_ids {
            match catalog::find_tool(id) {
                Some(spec) => selected.push(spec),
                None => return Err(format!("TOOL_NOT_FOUND: 未知工具 id：{id}")),
            }
        }
        selected
    };

    let tool_count = specs.len();
    let started = Instant::now();
    let tools: Vec<AiToolScanDto> = specs
        .par_iter()
        .enumerate()
        .map(|(tool_index, spec)| {
            let spec: &'static ToolSpec = spec;
            let progress = ScanProgress {
                tool_index,
                tool_count,
                tool_id: spec.id,
                scanned: AtomicU64::new(0),
                last_emit_ms: AtomicU64::new(0),
                sink,
            };
            scan_tool(spec, platform, lookup, &progress, include_heavy)
        })
        .collect();

    let total_cleanable_bytes = tools
        .iter()
        .map(|tool| tool.cleanable_bytes)
        .fold(0u64, |sum, value| sum.saturating_add(value));
    let total_display_only_bytes = tools
        .iter()
        .map(|tool| tool.display_only_bytes)
        .fold(0u64, |sum, value| sum.saturating_add(value));
    let scanned_at_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    Ok(AiCacheScanResult {
        platform: platform.as_str().to_string(),
        catalog_version: CATALOG_VERSION,
        scanned_at_secs,
        duration_ms: started.elapsed().as_millis() as u64,
        total_cleanable_bytes,
        total_display_only_bytes,
        tools,
        warnings: Vec::new(),
    })
}
