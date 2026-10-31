//! 清理实现：白名单精确匹配 + 路径规范化 + 符号链接/根目录拒绝 + 永久删除。
//!
//! v1 安全立场（plan §7 + Chief 裁决）：
//! - 前端传的 `path` 只当候选，最终以 catalog 白名单解析结果为准；
//! - 校验先判断符号链接（`symlink_metadata` 不 follow）再 canonicalize，避免链接被"解析掉"；
//! - 删除前紧邻一次复查，压缩 TOCTOU 窗口；
//! - 不调用 shell / PowerShell，统一用 Rust 原生 `fs`。

use super::catalog::{self, Platform};
use super::scan;
use super::{
    AiCacheCategory, AiCacheCleanProgressEvent, AiCacheCleanRequest, AiCacheCleanResult,
    AiCleanItem, AiCleanStatus,
};
use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, PathBuf};

/// 单次清理请求最多 64 项（规格 R10）。
pub const MAX_TARGETS_PER_REQUEST: usize = 64;

#[derive(Debug, Clone)]
pub struct ValidatedTarget {
    pub tool_id: String,
    pub category: AiCacheCategory,
    /// 前端传入的展示路径（删除时使用，保留软链根目录的原始形态）。
    pub display_path: String,
    pub lexical_path: PathBuf,
    pub canonical_path: PathBuf,
    /// 白名单模板末段含 `*`：只允许删除普通文件，目录一律拒绝（B1）。
    pub expects_file: bool,
}

#[derive(Debug)]
pub struct Rejection {
    pub status: AiCleanStatus,
    pub error: String,
}

impl Rejection {
    fn failed(code: &str, message: impl AsRef<str>) -> Self {
        Self {
            status: AiCleanStatus::Failed,
            error: format!("{code}: {}", message.as_ref()),
        }
    }

    fn skipped(code: &str, message: impl AsRef<str>) -> Self {
        Self {
            status: AiCleanStatus::Skipped,
            error: format!("{code}: {}", message.as_ref()),
        }
    }
}

fn trimmed_path(raw_path: &str) -> PathBuf {
    PathBuf::from(raw_path.trim())
}

/// 校验单个清理目标。任何失败都只影响该条，不打断整批。
pub fn validate_target(
    tool_id: &str,
    category: AiCacheCategory,
    raw_path: &str,
    platform: Platform,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<ValidatedTarget, Rejection> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err(Rejection::failed("INVALID_ARGUMENT", "路径不能为空"));
    }

    let lexical_path = trimmed_path(trimmed);
    if !lexical_path.is_absolute() {
        return Err(Rejection::failed("PATH_NOT_ABSOLUTE", "路径必须是绝对路径"));
    }
    for component in lexical_path.components() {
        match component {
            Component::ParentDir => {
                return Err(Rejection::failed("PATH_NOT_ALLOWED", "路径不允许包含 .."));
            }
            Component::CurDir => {
                return Err(Rejection::failed("PATH_NOT_ALLOWED", "路径不允许包含 ."));
            }
            _ => {}
        }
    }

    let Some(spec) = catalog::find_tool(tool_id) else {
        return Err(Rejection::failed("TOOL_NOT_FOUND", "未知工具 id"));
    };
    if !category.is_cleanable() {
        return Err(Rejection::failed(
            "CATEGORY_NOT_CLEANABLE",
            format!(
                "{} 类别在 v1 仅展示占用，不提供清理",
                catalog::category_meta(category).label
            ),
        ));
    }
    let Some(category_spec) = catalog::category_spec(spec, category) else {
        return Err(Rejection::failed(
            "CATEGORY_NOT_CLEANABLE",
            "该工具没有此可清理类别",
        ));
    };

    // 降级护栏（Chief 裁决 1）：根目录指向 home / home 上级 / 文件系统根时，
    // 该工具只检测不清理；即使路径命中白名单也必须拒绝。
    let roots = catalog::resolve_roots_with(spec, platform, lookup);
    let policy =
        catalog::tool_policy_for_roots(&roots, catalog::resolve_home(platform, lookup).as_deref());
    if !policy.cleanable {
        return Err(Rejection::failed(
            "TOOL_DISABLED",
            policy
                .warnings
                .first()
                .cloned()
                .unwrap_or_else(|| "该工具已降级为仅检测，不提供清理".to_string()),
        ));
    }

    let mut matched_template: Option<&'static str> = None;
    for root in &roots {
        for &template in category_spec.paths {
            if catalog::template_matches(&root.path, template, &lexical_path) {
                matched_template = Some(template);
                break;
            }
        }
        if matched_template.is_some() {
            break;
        }
    }
    let Some(matched_template) = matched_template else {
        return Err(Rejection::failed(
            "PATH_NOT_ALLOWED",
            "路径不在白名单目录内",
        ));
    };
    // 文件名型通配（如 codex 的 `logs_*.sqlite`）只允许删除普通文件：
    // 同名目录绝不能因为文件名匹配就被当成文件递归删除（B1）。
    let expects_file = matched_template.contains('*');

    // 先判断链接（不 follow），再 canonicalize —— 顺序不能反。
    let metadata = match fs::symlink_metadata(&lexical_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(Rejection::skipped("PATH_NOT_FOUND", "目标已不存在"));
        }
        Err(error) => {
            return Err(Rejection::failed(
                "PATH_NOT_READABLE",
                format!("无法读取目标：{error}"),
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(Rejection::skipped(
            "PATH_IS_SYMLINK",
            "已跳过符号链接，不删除链接目标",
        ));
    }
    if expects_file && !metadata.is_file() {
        return Err(Rejection::failed(
            "PATH_NOT_ALLOWED",
            "通配白名单项只允许删除普通文件，目标不是文件",
        ));
    }

    let canonical_path = match fs::canonicalize(&lexical_path) {
        Ok(path) => path,
        Err(error) => {
            return Err(Rejection::skipped(
                "PATH_NOT_FOUND",
                format!("目标已不存在：{error}"),
            ));
        }
    };

    let canonical_roots: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| fs::canonicalize(&root.path).ok())
        .collect();
    if canonical_roots.is_empty() {
        return Err(Rejection::failed(
            "ROOT_NOT_FOUND",
            "工具根目录不存在或不可访问",
        ));
    }
    if !canonical_roots
        .iter()
        .any(|root| catalog::is_within(&canonical_path, root))
    {
        return Err(Rejection::failed(
            "PATH_NOT_ALLOWED",
            "目标不在工具根目录内",
        ));
    }
    // 工具根目录本身永不删除。
    if canonical_roots
        .iter()
        .any(|root| catalog::path_eq(&canonical_path, root))
    {
        return Err(Rejection::failed("ROOT_PROTECTED", "禁止删除工具根目录"));
    }
    // 用户主目录 / 文件系统根永不删除。
    if let Some(home) = catalog::resolve_home(platform, lookup) {
        if let Ok(home_canonical) = fs::canonicalize(&home) {
            if catalog::path_eq(&canonical_path, &home_canonical) {
                return Err(Rejection::failed("ROOT_PROTECTED", "禁止删除用户主目录"));
            }
        }
    }
    if canonical_path.parent().is_none() {
        return Err(Rejection::failed(
            "ROOT_PROTECTED",
            "禁止删除文件系统根目录",
        ));
    }
    // 凭据 / 配置文件黑名单兜底：即使 catalog 误写也拒绝。
    if let Some(name) = canonical_path.file_name() {
        if catalog::is_forbidden_basename(&name.to_string_lossy()) {
            return Err(Rejection::failed(
                "PATH_NOT_ALLOWED",
                "凭据 / 配置文件不允许删除",
            ));
        }
    }

    Ok(ValidatedTarget {
        tool_id: tool_id.to_string(),
        category,
        display_path: trimmed.to_string(),
        lexical_path,
        canonical_path,
        expects_file,
    })
}

/// 去重：同一 canonical 路径只保留一次；祖先覆盖后代（避免重复记账）。
pub fn dedupe_targets(mut targets: Vec<ValidatedTarget>) -> Vec<ValidatedTarget> {
    targets.sort_by_key(|target| catalog::path_key(&target.canonical_path).len());
    let mut kept: Vec<ValidatedTarget> = Vec::new();
    for target in targets {
        let covered = kept.iter().any(|existing| {
            catalog::path_eq(&target.canonical_path, &existing.canonical_path)
                || catalog::is_within(&target.canonical_path, &existing.canonical_path)
        });
        if covered {
            continue;
        }
        kept.push(target);
    }
    kept
}

fn cleanup_item(
    tool_id: &str,
    category: AiCacheCategory,
    path: &str,
    bytes: u64,
    status: AiCleanStatus,
    error: Option<String>,
) -> AiCleanItem {
    AiCleanItem {
        tool_id: tool_id.to_string(),
        category,
        path: path.to_string(),
        bytes,
        status,
        error,
    }
}

/// 命令级校验：请求自洽（非空、≤64 项、工具存在、类别可清理、工具未被降级）。
fn validate_request(
    request: &AiCacheCleanRequest,
    platform: Platform,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<(), String> {
    if request.targets.is_empty() {
        return Err("INVALID_ARGUMENT: 清理目标不能为空".to_string());
    }
    if request.targets.len() > MAX_TARGETS_PER_REQUEST {
        return Err(format!(
            "INVALID_ARGUMENT: 单次清理最多 {MAX_TARGETS_PER_REQUEST} 项"
        ));
    }
    for target in &request.targets {
        let Some(spec) = catalog::find_tool(&target.tool_id) else {
            return Err(format!("TOOL_NOT_FOUND: 未知工具 id：{}", target.tool_id));
        };
        if !target.category.is_cleanable() {
            return Err(format!(
                "CATEGORY_NOT_CLEANABLE: {} 类别在 v1 仅展示，不提供清理",
                catalog::category_meta(target.category).label
            ));
        }
        if catalog::category_spec(spec, target.category).is_none() {
            return Err(format!(
                "CATEGORY_NOT_CLEANABLE: 工具 {} 没有 {} 类别",
                spec.name,
                catalog::category_meta(target.category).label
            ));
        }
        // 降级工具在命令级就整批拒绝，保证"一个字节都不删"。
        let policy = catalog::tool_policy(spec, platform, lookup);
        if !policy.cleanable {
            return Err(format!(
                "TOOL_DISABLED: 工具 {} {}",
                spec.name,
                policy
                    .warnings
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "已降级为仅检测，不提供清理".to_string())
            ));
        }
    }
    Ok(())
}

fn progress_event(
    tool_id: &str,
    path: &str,
    deleted_count: u64,
    skipped_count: u64,
    failed_count: u64,
    freed_bytes: u64,
) -> AiCacheCleanProgressEvent {
    AiCacheCleanProgressEvent {
        tool_id: tool_id.to_string(),
        path: path.to_string(),
        deleted_count,
        skipped_count,
        failed_count,
        processed_count: deleted_count
            .saturating_add(skipped_count)
            .saturating_add(failed_count),
        freed_bytes,
    }
}

/// 清理入口。dry-run 与真实删除共用同一套校验；单条失败不打断整体。
pub fn clean_all(
    platform: Platform,
    lookup: &dyn Fn(&str) -> Option<OsString>,
    request: AiCacheCleanRequest,
    sink: &dyn Fn(AiCacheCleanProgressEvent),
) -> Result<AiCacheCleanResult, String> {
    validate_request(&request, platform, lookup)?;

    let dry_run = request.dry_run;
    let mut rejected: Vec<AiCleanItem> = Vec::new();
    let mut validated: Vec<ValidatedTarget> = Vec::new();

    for target in &request.targets {
        match validate_target(
            &target.tool_id,
            target.category,
            &target.path,
            platform,
            lookup,
        ) {
            Ok(value) => validated.push(value),
            Err(rejection) => rejected.push(cleanup_item(
                &target.tool_id,
                target.category,
                target.path.trim(),
                0,
                rejection.status,
                Some(rejection.error),
            )),
        }
    }

    let mut warnings: Vec<String> = Vec::new();
    for item in &rejected {
        match item.status {
            AiCleanStatus::Skipped => warnings.push(format!(
                "已跳过 {}：{}",
                item.path,
                item.error.as_deref().unwrap_or("目标不可用")
            )),
            AiCleanStatus::Failed => warnings.push(format!(
                "校验未通过 {}：{}",
                item.path,
                item.error.as_deref().unwrap_or("目标不可用")
            )),
            _ => {}
        }
    }

    let kept = dedupe_targets(validated);
    let mut accepted: Vec<AiCleanItem> = Vec::new();
    let mut freed_bytes: u64 = 0;
    let mut deleted_count: u64 = 0;
    let mut progress_skipped: u64 = 0;
    let mut progress_failed: u64 = 0;

    // 真实清理时，校验阶段即判定跳过/失败的目标也计入进度：
    // 前端进度分母是"整批目标数"，否则进度条永远到不了 100%。
    if !dry_run {
        for item in &rejected {
            match item.status {
                AiCleanStatus::Skipped => progress_skipped = progress_skipped.saturating_add(1),
                AiCleanStatus::Failed => progress_failed = progress_failed.saturating_add(1),
                _ => continue,
            }
            sink(progress_event(
                &item.tool_id,
                &item.path,
                deleted_count,
                progress_skipped,
                progress_failed,
                freed_bytes,
            ));
        }
    }

    for target in kept {
        let outcome = scan::measure_bytes(&target.lexical_path);

        if dry_run {
            freed_bytes = freed_bytes.saturating_add(outcome.bytes);
            accepted.push(cleanup_item(
                &target.tool_id,
                target.category,
                &target.display_path,
                outcome.bytes,
                AiCleanStatus::WouldDelete,
                None,
            ));
            continue;
        }

        // 删除前最后一次复查：压缩 TOCTOU 窗口。
        let metadata = match fs::symlink_metadata(&target.lexical_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                progress_skipped = progress_skipped.saturating_add(1);
                accepted.push(cleanup_item(
                    &target.tool_id,
                    target.category,
                    &target.display_path,
                    0,
                    AiCleanStatus::Skipped,
                    Some("PATH_NOT_FOUND: 目标已不存在".to_string()),
                ));
                sink(progress_event(
                    &target.tool_id,
                    &target.display_path,
                    deleted_count,
                    progress_skipped,
                    progress_failed,
                    freed_bytes,
                ));
                continue;
            }
            Err(error) => {
                progress_failed = progress_failed.saturating_add(1);
                accepted.push(cleanup_item(
                    &target.tool_id,
                    target.category,
                    &target.display_path,
                    0,
                    AiCleanStatus::Failed,
                    Some(format!("PATH_NOT_READABLE: 无法读取目标：{error}")),
                ));
                sink(progress_event(
                    &target.tool_id,
                    &target.display_path,
                    deleted_count,
                    progress_skipped,
                    progress_failed,
                    freed_bytes,
                ));
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            progress_skipped = progress_skipped.saturating_add(1);
            accepted.push(cleanup_item(
                &target.tool_id,
                target.category,
                &target.display_path,
                0,
                AiCleanStatus::Skipped,
                Some("PATH_IS_SYMLINK: 已跳过符号链接，不删除链接目标".to_string()),
            ));
            sink(progress_event(
                &target.tool_id,
                &target.display_path,
                deleted_count,
                progress_skipped,
                progress_failed,
                freed_bytes,
            ));
            continue;
        }
        // 通配白名单只允许文件：检查后被换成目录 → 跳过（B1 的 TOCTOU 兜底）。
        if target.expects_file && !metadata.is_file() {
            progress_skipped = progress_skipped.saturating_add(1);
            accepted.push(cleanup_item(
                &target.tool_id,
                target.category,
                &target.display_path,
                0,
                AiCleanStatus::Skipped,
                Some("PATH_CHANGED: 通配白名单项目标已不是普通文件，已跳过".to_string()),
            ));
            sink(progress_event(
                &target.tool_id,
                &target.display_path,
                deleted_count,
                progress_skipped,
                progress_failed,
                freed_bytes,
            ));
            continue;
        }
        // 路径在扫描/预览后被替换成指向白名单外的链接 → 跳过。
        match fs::canonicalize(&target.lexical_path) {
            Ok(canonical) if catalog::path_eq(&canonical, &target.canonical_path) => {}
            _ => {
                progress_skipped = progress_skipped.saturating_add(1);
                accepted.push(cleanup_item(
                    &target.tool_id,
                    target.category,
                    &target.display_path,
                    0,
                    AiCleanStatus::Skipped,
                    Some("PATH_CHANGED: 目标在清理前发生变化，已跳过".to_string()),
                ));
                sink(progress_event(
                    &target.tool_id,
                    &target.display_path,
                    deleted_count,
                    progress_skipped,
                    progress_failed,
                    freed_bytes,
                ));
                continue;
            }
        }

        let result = if metadata.is_dir() {
            fs::remove_dir_all(&target.lexical_path)
        } else {
            fs::remove_file(&target.lexical_path)
        };

        match result {
            Ok(()) => {
                deleted_count = deleted_count.saturating_add(1);
                freed_bytes = freed_bytes.saturating_add(outcome.bytes);
                accepted.push(cleanup_item(
                    &target.tool_id,
                    target.category,
                    &target.display_path,
                    outcome.bytes,
                    AiCleanStatus::Deleted,
                    None,
                ));
                sink(progress_event(
                    &target.tool_id,
                    &target.display_path,
                    deleted_count,
                    progress_skipped,
                    progress_failed,
                    freed_bytes,
                ));
            }
            Err(error) => {
                progress_failed = progress_failed.saturating_add(1);
                accepted.push(cleanup_item(
                    &target.tool_id,
                    target.category,
                    &target.display_path,
                    0,
                    AiCleanStatus::Failed,
                    Some(format!("DELETE_FAILED: 删除失败：{error}")),
                ));
                sink(progress_event(
                    &target.tool_id,
                    &target.display_path,
                    deleted_count,
                    progress_skipped,
                    progress_failed,
                    freed_bytes,
                ));
            }
        }
    }

    // 删除阶段的跳过/失败同样要进 warnings（不再只报校验阶段的问题）。
    for item in &accepted {
        match item.status {
            AiCleanStatus::Skipped => warnings.push(format!(
                "已跳过 {}：{}",
                item.path,
                item.error.as_deref().unwrap_or("目标不可用")
            )),
            AiCleanStatus::Failed => warnings.push(format!(
                "清理失败 {}：{}",
                item.path,
                item.error.as_deref().unwrap_or("未知原因")
            )),
            _ => {}
        }
    }

    let mut items = rejected;
    items.extend(accepted);
    let skipped_count = items
        .iter()
        .filter(|item| item.status == AiCleanStatus::Skipped)
        .count() as u64;
    let failed_count = items
        .iter()
        .filter(|item| item.status == AiCleanStatus::Failed)
        .count() as u64;

    Ok(AiCacheCleanResult {
        dry_run,
        freed_bytes,
        deleted_count,
        skipped_count,
        failed_count,
        items,
        warnings,
    })
}
