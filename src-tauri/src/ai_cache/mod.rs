//! AI 智能体缓存清理（v1）。
//!
//! 模块划分（按 plan.md §6.1）：
//! - `catalog`：9 个 AI 工具的路径白名单（数据驱动、可单测）
//! - `scan`：体积扫描（`spawn_blocking` 中执行，只累加字节，不保留文件树）
//! - `clean`：清理前校验 + 永久删除（v1 零新增依赖，不引入回收站）
//!
//! 安全边界（Chief 裁决，实现不可打折）：
//! - 前端只提交 `toolId + category + 绝对路径`，后端用 catalog 白名单**精确匹配**；
//! - 只允许清理 `cache` / `logs`；`sessions` / `model_cache` 仅展示占用，不提供删除入口；
//! - 符号链接 / junction、工具根、用户主目录、文件系统根、凭据文件名一律拒绝；
//! - v1 只做永久删除；确认弹窗必须逐条列出绝对路径（前端实现）。

mod catalog;
mod clean;
mod scan;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use tauri::Emitter;

/// 白名单版本。路径表变更时必须递增，扫描结果会带回该值用于排查。
pub const CATALOG_VERSION: u32 = 1;

/// 可清理类别（v1 固定 4 类，其中只有 cache / logs 允许清理）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiCacheCategory {
    Cache,
    Logs,
    Sessions,
    ModelCache,
}

impl AiCacheCategory {
    /// 该类别是否允许清理。v1 只允许 cache / logs，其余仅展示。
    pub fn is_cleanable(self) -> bool {
        matches!(self, Self::Cache | Self::Logs)
    }
}

/// 类别风险等级（用于前端徽标，不参与删除决策）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiRisk {
    Safe,
    History,
    Redownload,
}

/// 白名单目标的文件系统类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiPathKind {
    Dir,
    File,
}

// ── 扫描响应 DTO ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCachePathDto {
    /// 绝对路径，仅用于展示与前端回传（后端会重新校验）。
    pub path: String,
    pub bytes: u64,
    pub kind: AiPathKind,
    pub last_modified_ms: u64,
    /// 最近 10 分钟内有文件修改：默认不勾选，允许用户手动勾选。
    pub in_use: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheCategoryDto {
    pub category: AiCacheCategory,
    pub label: String,
    pub risk: AiRisk,
    /// false = 仅展示，不提供任何删除入口。
    pub cleanable: bool,
    pub default_selected: bool,
    pub description: String,
    pub paths: Vec<AiCachePathDto>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheRootDto {
    pub path: String,
    pub exists: bool,
    pub categories: Vec<AiCacheCategoryDto>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolScanDto {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub detected: bool,
    pub roots: Vec<AiCacheRootDto>,
    /// 仅统计可清理类别（cache / logs）。
    pub cleanable_bytes: u64,
    /// 仅统计展示类别（sessions / model_cache）。
    pub display_only_bytes: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheScanResult {
    pub platform: String,
    pub catalog_version: u32,
    pub scanned_at_secs: u64,
    pub duration_ms: u64,
    pub total_cleanable_bytes: u64,
    pub total_display_only_bytes: u64,
    pub tools: Vec<AiToolScanDto>,
    pub warnings: Vec<String>,
}

// ── catalog 响应 DTO（页面首屏用，不扫体积） ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolCatalogDto {
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// 解析出的绝对路径（含不存在的，便于空态展示"已检查路径"）。
    pub roots: Vec<String>,
    pub detected: bool,
    pub protected: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheCatalog {
    pub platform: String,
    pub catalog_version: u32,
    pub tools: Vec<AiToolCatalogDto>,
}

// ── 请求 ──

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheScanRequest {
    /// 空 = 扫描全部工具。
    #[serde(default)]
    pub tool_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheCleanTarget {
    pub tool_id: String,
    pub category: AiCacheCategory,
    /// 扫描返回的绝对路径；后端只把它当"候选"，最终以白名单为准。
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheCleanRequest {
    pub targets: Vec<AiCacheCleanTarget>,
    /// 缺省 true：任何调用方忘了传都只会预览，不会真的删除。
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

// ── 清理响应 DTO ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiCleanStatus {
    WouldDelete,
    Deleted,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCleanItem {
    pub tool_id: String,
    pub category: AiCacheCategory,
    pub path: String,
    pub bytes: u64,
    pub status: AiCleanStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheCleanResult {
    pub dry_run: bool,
    pub freed_bytes: u64,
    pub deleted_count: u64,
    pub skipped_count: u64,
    pub failed_count: u64,
    pub items: Vec<AiCleanItem>,
    pub warnings: Vec<String>,
}

// ── 进度事件 ──

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheScanProgressEvent {
    pub tool_id: String,
    pub current_path: String,
    pub scanned_entries: u64,
    pub tool_index: usize,
    pub tool_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheCleanProgressEvent {
    pub tool_id: String,
    pub path: String,
    pub deleted_count: u64,
    pub freed_bytes: u64,
}

// ── Tauri commands ──

fn env_lookup(key: &str) -> Option<OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

/// 只读：返回 9 个工具的预期根目录（毫秒级，不扫描体积）。
#[tauri::command]
pub fn get_ai_cache_catalog() -> Result<AiCacheCatalog, String> {
    let platform = catalog::current_platform();
    Ok(catalog::build_catalog(platform, &env_lookup))
}

/// 只读：自动检测 + 扫描可清理/仅展示类别体积。
///
/// `spawn_blocking` 保证大目录扫描不阻塞 WebView 事件循环；进度事件按 plan §6.8 节流。
#[tauri::command]
pub async fn scan_ai_cache_usage(
    window: tauri::Window,
    request: AiCacheScanRequest,
) -> Result<AiCacheScanResult, String> {
    let platform = catalog::current_platform();
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |event: AiCacheScanProgressEvent| {
            let _ = window.emit("ai-cache-scan-progress", event);
        };
        scan::scan_all(platform, &env_lookup, &request.tool_ids, &emit)
    })
    .await
    .map_err(|error| format!("SCAN_FAILED: 扫描任务执行失败：{error}"))?
}

/// 写：清理（前端只传 toolId + category + 绝对路径）。
///
/// `dryRun` 默认 true；dry-run 与真实删除共用同一套白名单校验。
/// v1 为永久删除，不引入回收站依赖。
#[tauri::command]
pub async fn clean_ai_caches(
    window: tauri::Window,
    request: AiCacheCleanRequest,
) -> Result<AiCacheCleanResult, String> {
    let platform = catalog::current_platform();
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |event: AiCacheCleanProgressEvent| {
            let _ = window.emit("ai-cache-clean-progress", event);
        };
        clean::clean_all(platform, &env_lookup, request, &emit)
    })
    .await
    .map_err(|error| format!("CLEAN_FAILED: 清理任务执行失败：{error}"))?
}
