//! AI 工具路径白名单（数据驱动）。
//!
//! 设计原则（plan.md §5）：
//! - 只存"逻辑基址 + 相对子路径"，运行时按平台展开，禁止硬编码用户名；
//! - 同一工具三平台只有根目录不同，因此 `RootSpec` 用 `platforms` 过滤；
//! - 可清理路径是**封闭白名单**：只有 `CategorySpec.paths` 中、`cache` / `logs` 两类
//!   且实际存在的目录才允许删除；其余内容（配置、凭据、扩展、状态库）永不进入白名单；
//! - 标 ⚠️ 未实测的路径按 plan 要求先不进入白名单（注释留 TODO）。

use super::{AiCacheCatalog, AiCacheCategory, AiRisk, AiToolCatalogDto, CATALOG_VERSION};
use globset::Glob;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

// ── 平台与基址 ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::MacOs => "macos",
            Platform::Linux => "linux",
            Platform::Windows => "windows",
        }
    }
}

pub fn current_platform() -> Platform {
    if cfg!(target_os = "macos") {
        Platform::MacOs
    } else if cfg!(target_os = "windows") {
        Platform::Windows
    } else {
        Platform::Linux
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // 基址枚举保留完整平台集合，v2 新增工具表条目时直接复用。
pub enum BaseDir {
    Home,
    MacApplicationSupport,
    XdgConfig,
    XdgData,
    XdgCache,
    AppDataRoaming,
    LocalAppData,
}

pub struct RootSpec {
    pub platforms: &'static [Platform],
    pub base: BaseDir,
    pub relative: &'static str,
    pub env_override: Option<&'static str>,
}

pub struct CategorySpec {
    pub category: AiCacheCategory,
    pub paths: &'static [&'static str],
}

pub struct ToolSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    pub roots: &'static [RootSpec],
    pub categories: &'static [CategorySpec],
    pub protected: &'static [&'static str],
}

/// 类别元信息（文案 / 风险等级 / 是否默认勾选）。
#[derive(Debug, Clone, Copy)]
pub struct CategoryMeta {
    pub label: &'static str,
    pub risk: AiRisk,
    pub default_selected: bool,
    pub description: &'static str,
}

pub fn category_meta(category: AiCacheCategory) -> CategoryMeta {
    match category {
        AiCacheCategory::Cache => CategoryMeta {
            label: "缓存",
            risk: AiRisk::Safe,
            default_selected: true,
            description: "可安全删除，工具会在下次使用时自动重建，不影响会话记录与配置。",
        },
        AiCacheCategory::Logs => CategoryMeta {
            label: "日志",
            risk: AiRisk::Safe,
            default_selected: true,
            description: "运行日志与调试残留，删除后失去历史日志，工具可正常运行。",
        },
        AiCacheCategory::Sessions => CategoryMeta {
            label: "会话记录",
            risk: AiRisk::History,
            default_selected: false,
            description: "聊天记录、编辑历史与工作区状态，删除后不可恢复；v1 仅展示，不提供清理。",
        },
        AiCacheCategory::ModelCache => CategoryMeta {
            label: "模型文件",
            risk: AiRisk::Redownload,
            default_selected: false,
            description: "模型权重文件，删除后需要重新下载；v1 仅展示，不提供清理。",
        },
    }
}

// ── 环境变量 / 基址解析（env 读取可注入，便于单测） ──

fn var_path(lookup: &dyn Fn(&str) -> Option<OsString>, key: &str) -> Option<PathBuf> {
    lookup(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn windows_home_from_drive(lookup: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let drive = lookup("HOMEDRIVE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    let rest = lookup("HOMEPATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)?;
    let mut home = drive;
    home.push(rest);
    if home.is_absolute() {
        Some(home)
    } else {
        None
    }
}

pub fn resolve_home(
    platform: Platform,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    if platform == Platform::Windows {
        var_path(lookup, "USERPROFILE").or_else(|| windows_home_from_drive(lookup))
    } else {
        var_path(lookup, "HOME")
    }
}

fn resolve_base(
    base: BaseDir,
    platform: Platform,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    match base {
        BaseDir::Home => resolve_home(platform, lookup),
        BaseDir::MacApplicationSupport => {
            if platform == Platform::MacOs {
                resolve_home(platform, lookup)
                    .map(|home| home.join("Library").join("Application Support"))
            } else {
                None
            }
        }
        BaseDir::XdgConfig => var_path(lookup, "XDG_CONFIG_HOME")
            .or_else(|| resolve_home(platform, lookup).map(|home| home.join(".config"))),
        BaseDir::XdgData => var_path(lookup, "XDG_DATA_HOME").or_else(|| {
            resolve_home(platform, lookup).map(|home| home.join(".local").join("share"))
        }),
        BaseDir::XdgCache => var_path(lookup, "XDG_CACHE_HOME")
            .or_else(|| resolve_home(platform, lookup).map(|home| home.join(".cache"))),
        BaseDir::AppDataRoaming => var_path(lookup, "APPDATA").or_else(|| {
            resolve_home(platform, lookup).map(|home| home.join("AppData").join("Roaming"))
        }),
        BaseDir::LocalAppData => var_path(lookup, "LOCALAPPDATA").or_else(|| {
            resolve_home(platform, lookup).map(|home| home.join("AppData").join("Local"))
        }),
    }
}

// ── 路径归一化辅助（Windows 大小写不敏感） ──

#[cfg(windows)]
pub fn path_key(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    while value.ends_with('\\') && value.len() > 3 {
        value.pop();
    }
    value.to_lowercase()
}

#[cfg(not(windows))]
pub fn path_key(path: &Path) -> String {
    let mut value = path.to_string_lossy().to_string();
    while value.len() > 1 && value.ends_with('/') {
        value.pop();
    }
    value
}

pub fn path_eq(a: &Path, b: &Path) -> bool {
    path_key(a) == path_key(b)
}

/// 严格后代判断：`child` 必须在 `parent` 之下（不相等）。
pub fn is_within(child: &Path, parent: &Path) -> bool {
    let child_key = path_key(child);
    let parent_key = path_key(parent);
    if child_key == parent_key {
        return false;
    }
    let separator = std::path::MAIN_SEPARATOR;
    let prefix = if parent_key.ends_with(separator) {
        parent_key
    } else {
        format!("{parent_key}{separator}")
    };
    child_key.starts_with(&prefix)
}

// ── 解析结果 ──

#[derive(Debug, Clone)]
pub struct ResolvedRoot {
    pub path: PathBuf,
    pub exists: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub path: PathBuf,
}

fn push_unique_root(
    out: &mut Vec<ResolvedRoot>,
    lexical_seen: &mut HashSet<String>,
    canonical_seen: &mut HashSet<String>,
    path: PathBuf,
) {
    let lexical_key = path_key(&path);
    if lexical_seen.contains(&lexical_key) {
        return;
    }
    let exists = path.exists();
    let canonical_key = fs::canonicalize(&path).ok().map(|value| path_key(&value));
    if let Some(key) = canonical_key.as_ref() {
        if canonical_seen.contains(key) {
            return;
        }
    }
    lexical_seen.insert(lexical_key);
    if let Some(key) = canonical_key {
        canonical_seen.insert(key);
    }
    out.push(ResolvedRoot { path, exists });
}

/// 解析工具根目录；`lookup` 用于读取环境变量（生产传 `env::var_os`，单测传假查找器）。
pub fn resolve_roots_with(
    spec: &ToolSpec,
    platform: Platform,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Vec<ResolvedRoot> {
    let mut roots: Vec<ResolvedRoot> = Vec::new();
    let mut lexical_seen: HashSet<String> = HashSet::new();
    let mut canonical_seen: HashSet<String> = HashSet::new();

    for root_spec in spec.roots {
        if !root_spec.platforms.contains(&platform) {
            continue;
        }

        if let Some(env_key) = root_spec.env_override {
            if let Some(overridden) = var_path(lookup, env_key) {
                push_unique_root(
                    &mut roots,
                    &mut lexical_seen,
                    &mut canonical_seen,
                    overridden,
                );
                continue;
            }
        }

        let Some(base) = resolve_base(root_spec.base, platform, lookup) else {
            continue;
        };
        let mut path = base;
        if !root_spec.relative.is_empty() {
            path.push(root_spec.relative);
        }
        push_unique_root(&mut roots, &mut lexical_seen, &mut canonical_seen, path);
    }

    roots
}

/// 展开某个根目录下、某个类别的实际目标。
///
/// 相对路径末段允许一个 `*`（如 `logs_*.sqlite`），只在根级目录内按文件名匹配，
/// 不接受跨目录通配（避免 `**` 之类的逃逸面）。
pub fn expand_targets(root: &Path, category: &CategorySpec) -> Vec<ResolvedTarget> {
    let mut targets = Vec::new();
    for &template in category.paths {
        if template.contains('*') {
            let (parent_rel, pattern) = match template.rsplit_once('/') {
                Some((parent, pattern)) => (parent, pattern),
                None => ("", template),
            };
            let directory = if parent_rel.is_empty() {
                root.to_path_buf()
            } else {
                root.join(parent_rel)
            };
            let Ok(glob) = Glob::new(pattern) else {
                continue;
            };
            let matcher = glob.compile_matcher();
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            // 文件名型通配只接受**普通文件**：同名目录不能因为名字匹配就被当成
            // 文件项（否则 clean 会走 remove_dir_all 递归删掉整个目录，B1）。
            let mut matches: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_type()
                        .map(|file_type| file_type.is_file())
                        .unwrap_or(false)
                })
                .filter(|entry| matcher.is_match(Path::new(&entry.file_name())))
                .map(|entry| entry.path())
                .collect();
            matches.sort();
            for path in matches {
                targets.push(ResolvedTarget { path });
            }
        } else {
            targets.push(ResolvedTarget {
                path: root.join(template),
            });
        }
    }
    targets
}

// ── 工具清理策略（Chief 裁决 1：override 指向敏感位置时降级为仅检测） ──

/// 单个工具的清理策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolPolicy {
    /// false = 该工具只做检测/展示，任何清理请求都必须被拒绝。
    pub cleanable: bool,
    /// 降级原因（展示在 catalog 与扫描结果的 warnings 里）。
    pub warnings: Vec<String>,
}

/// 根目录是否落在不允许清理的敏感位置：用户主目录本身、主目录的祖先、文件系统根。
///
/// 用户可以把 `CLAUDE_CONFIG_DIR` / `CODEX_HOME` 等 override 指到任意目录；
/// 一旦指到 `$HOME`，白名单里的 `backups` / `plans` / `logs` 就会变成家目录下的同名目录，
/// 删除它们是不可接受的，因此该工具整体降级为仅检测。
fn unsafe_root_reason(root: &Path, home: Option<&Path>) -> Option<String> {
    let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if let Some(home) = home {
        let canonical_home = fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
        if path_eq(&canonical_root, &canonical_home) {
            return Some(format!(
                "根目录指向用户主目录，已降级为仅检测（不提供清理）：{}",
                root.display()
            ));
        }
        if is_within(&canonical_home, &canonical_root) {
            return Some(format!(
                "根目录是用户主目录的上级，已降级为仅检测（不提供清理）：{}",
                root.display()
            ));
        }
    }
    if canonical_root.parent().is_none() {
        return Some(format!(
            "根目录是文件系统根，已降级为仅检测（不提供清理）：{}",
            root.display()
        ));
    }
    None
}

/// 根据已解析的根目录计算工具策略（可单测，不读真实环境变量）。
pub fn tool_policy_for_roots(roots: &[ResolvedRoot], home: Option<&Path>) -> ToolPolicy {
    let warnings: Vec<String> = roots
        .iter()
        .filter_map(|root| unsafe_root_reason(&root.path, home))
        .collect();
    ToolPolicy {
        cleanable: warnings.is_empty(),
        warnings,
    }
}

/// 解析根目录并计算清理策略（生产命令与测试共用）。
pub fn tool_policy(
    spec: &ToolSpec,
    platform: Platform,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> ToolPolicy {
    let roots = resolve_roots_with(spec, platform, lookup);
    tool_policy_for_roots(&roots, resolve_home(platform, lookup).as_deref())
}

/// 判断候选路径是否精确命中某个白名单模板。
///
/// 与 `expand_targets` 的区别：这里**不要求目标存在**，清理校验用它来保证
/// 已删除目标（幂等重试）仍然能被识别为白名单路径，而不是被误判为越界。
pub fn template_matches(root: &Path, template: &str, candidate: &Path) -> bool {
    if !template.contains('*') {
        return path_eq(&root.join(template), candidate);
    }

    let (parent_rel, pattern) = match template.rsplit_once('/') {
        Some((parent, pattern)) => (parent, pattern),
        None => ("", template),
    };
    let directory = if parent_rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(parent_rel)
    };
    let Some(parent) = candidate.parent() else {
        return false;
    };
    if !path_eq(parent, &directory) {
        return false;
    }
    let Some(name) = candidate.file_name() else {
        return false;
    };
    match Glob::new(pattern) {
        Ok(glob) => glob.compile_matcher().is_match(name),
        Err(_) => false,
    }
}

pub fn tools() -> &'static [ToolSpec] {
    TOOLS
}

pub fn find_tool(id: &str) -> Option<&'static ToolSpec> {
    tools().iter().find(|tool| tool.id == id)
}

pub fn category_spec(spec: &ToolSpec, category: AiCacheCategory) -> Option<&'static CategorySpec> {
    spec.categories
        .iter()
        .find(|candidate| candidate.category == category)
}

/// 全局兜底黑名单：即使误写进 catalog，也绝不允许删除这些 basename。
pub const FORBIDDEN_BASENAMES: &[&str] = &[
    ".credentials.json",
    "auth.json",
    "oauth_creds.json",
    "credentials.json",
    "hosts.json",
    "config.json",
    "config.toml",
    "settings.json",
    "settings.local.json",
    ".claude.json",
    "state.vscdb",
    "state.vscdb.backup",
    "id_ed25519",
    "history.jsonl",
    ".env",
];

pub fn is_forbidden_basename(name: &str) -> bool {
    let lower = name.to_lowercase();
    FORBIDDEN_BASENAMES.contains(&lower.as_str())
        || lower.ends_with(".key")
        || lower.ends_with(".pem")
}

/// 构造首屏 catalog（只做路径拼接 + exists 判断，不扫描体积）。
pub fn build_catalog(
    platform: Platform,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> AiCacheCatalog {
    let tools = tools()
        .iter()
        .map(|spec| {
            let roots = resolve_roots_with(spec, platform, lookup);
            let detected = roots.iter().any(|root| root.exists);
            let policy = tool_policy_for_roots(&roots, resolve_home(platform, lookup).as_deref());
            AiToolCatalogDto {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                vendor: spec.vendor.to_string(),
                roots: roots
                    .iter()
                    .map(|root| root.path.display().to_string())
                    .collect(),
                detected,
                protected: spec.protected.iter().map(|item| item.to_string()).collect(),
                cleanable: policy.cleanable,
                warnings: policy.warnings,
            }
        })
        .collect();

    AiCacheCatalog {
        platform: platform.as_str().to_string(),
        catalog_version: CATALOG_VERSION,
        tools,
    }
}

// ── 白名单常量表 ──

const ALL_PLATFORMS: &[Platform] = &[Platform::MacOs, Platform::Linux, Platform::Windows];
const MAC_ONLY: &[Platform] = &[Platform::MacOs];
const LINUX_ONLY: &[Platform] = &[Platform::Linux];
const WINDOWS_ONLY: &[Platform] = &[Platform::Windows];

const TOOLS: &[ToolSpec] = &[
    // ── 1. Claude Code ──
    ToolSpec {
        id: "claude-code",
        name: "Claude Code",
        vendor: "Anthropic",
        roots: &[RootSpec {
            platforms: ALL_PLATFORMS,
            base: BaseDir::Home,
            relative: ".claude",
            env_override: Some("CLAUDE_CONFIG_DIR"),
        }],
        categories: &[
            CategorySpec {
                category: AiCacheCategory::Cache,
                paths: &[
                    "cache",
                    "shell-snapshots",
                    "paste-cache",
                    "image-cache",
                    "todos",
                    "statsig",
                    "logs",
                    "plans",
                    "session-env",
                    "backups",
                    "usage-data",
                ],
            },
            CategorySpec {
                category: AiCacheCategory::Logs,
                paths: &["debug"],
            },
            CategorySpec {
                category: AiCacheCategory::Sessions,
                paths: &[
                    "projects",
                    "file-history",
                    "tasks",
                    "uploads",
                    "dev-mods",
                    "feedback",
                    "feedback-bundles",
                    "history.jsonl",
                ],
            },
        ],
        protected: &[
            ".credentials.json",
            "settings.json",
            "settings.local.json",
            "plugins",
            "skills",
            "agents",
            "commands",
            "rules",
            "output-styles",
            "workflows",
            "themes",
            "keybindings.json",
            "agent-memory",
            "jobs",
            "daemon",
            "CLAUDE.md",
            "remote-settings.json",
            "policy-limits.json",
            "stats-cache.json",
        ],
    },
    // ── 2. Claude Desktop（Electron 标准布局；⚠️ 子目录待真机复核） ──
    ToolSpec {
        id: "claude-desktop",
        name: "Claude Desktop",
        vendor: "Anthropic",
        roots: &[
            RootSpec {
                platforms: MAC_ONLY,
                base: BaseDir::MacApplicationSupport,
                relative: "Claude",
                env_override: None,
            },
            RootSpec {
                platforms: LINUX_ONLY,
                base: BaseDir::XdgConfig,
                relative: "Claude",
                env_override: None,
            },
            RootSpec {
                platforms: WINDOWS_ONLY,
                base: BaseDir::AppDataRoaming,
                relative: "Claude",
                env_override: None,
            },
        ],
        categories: &[
            CategorySpec {
                category: AiCacheCategory::Cache,
                paths: &[
                    "Cache",
                    "Code Cache",
                    "CachedData",
                    "GPUCache",
                    "DawnGraphiteCache",
                    "DawnWebGPUCache",
                    "GrShaderCache",
                    "ShaderCache",
                    "Service Worker/CacheStorage",
                    "Service Worker/ScriptCache",
                ],
            },
            CategorySpec {
                category: AiCacheCategory::Logs,
                paths: &["Crashpad", "logs"],
            },
            CategorySpec {
                category: AiCacheCategory::Sessions,
                paths: &["IndexedDB", "Local Storage"],
            },
        ],
        protected: &[
            "claude_desktop_config.json",
            "Preferences",
            "Cookies",
            "Network",
        ],
    },
    // ── 3. Codex CLI ──
    ToolSpec {
        id: "codex",
        name: "Codex CLI",
        vendor: "OpenAI",
        roots: &[RootSpec {
            platforms: ALL_PLATFORMS,
            base: BaseDir::Home,
            relative: ".codex",
            env_override: Some("CODEX_HOME"),
        }],
        categories: &[
            CategorySpec {
                category: AiCacheCategory::Cache,
                paths: &[
                    "cache",
                    ".tmp",
                    "tmp",
                    "plugins/cache",
                    "ambient-suggestions",
                ],
            },
            CategorySpec {
                category: AiCacheCategory::Logs,
                paths: &["logs_*.sqlite"],
            },
            CategorySpec {
                category: AiCacheCategory::Sessions,
                paths: &[
                    "sessions",
                    "archived_sessions",
                    "session_index.jsonl",
                    "history.jsonl",
                ],
            },
        ],
        protected: &[
            "auth.json",
            "config.toml",
            "AGENTS.md",
            "state_*.sqlite",
            "goals_*.sqlite",
            "sqlite",
            "skills",
            "rules",
            "browser",
            "computer-use",
            "pets",
            ".sandbox-bin",
            ".sandbox-secrets",
            ".codex-global-state.json",
        ],
    },
    // ── 4. Cursor ──
    ToolSpec {
        id: "cursor",
        name: "Cursor",
        vendor: "Anysphere",
        roots: &[
            RootSpec {
                platforms: MAC_ONLY,
                base: BaseDir::MacApplicationSupport,
                relative: "Cursor",
                env_override: None,
            },
            RootSpec {
                platforms: LINUX_ONLY,
                base: BaseDir::XdgConfig,
                relative: "Cursor",
                env_override: None,
            },
            RootSpec {
                platforms: WINDOWS_ONLY,
                base: BaseDir::AppDataRoaming,
                relative: "Cursor",
                env_override: None,
            },
        ],
        categories: &[
            CategorySpec {
                category: AiCacheCategory::Cache,
                paths: &[
                    "Cache",
                    "Code Cache",
                    "CachedData",
                    "GPUCache",
                    "DawnGraphiteCache",
                    "DawnWebGPUCache",
                    "GrShaderCache",
                    "ShaderCache",
                    "CachedExtensionVSIXs",
                    "CachedProfilesData",
                    "Service Worker/CacheStorage",
                    "Service Worker/ScriptCache",
                ],
            },
            CategorySpec {
                category: AiCacheCategory::Logs,
                paths: &["Crashpad", "logs"],
            },
            CategorySpec {
                category: AiCacheCategory::Sessions,
                paths: &["User/History", "User/workspaceStorage"],
            },
        ],
        protected: &[
            "User/globalStorage",
            "User/settings.json",
            "User/keybindings.json",
            "extensions",
        ],
    },
    // ── 5. GitHub Copilot（VS Code 扩展，多编辑器根） ──
    ToolSpec {
        id: "copilot-vscode",
        name: "GitHub Copilot（VS Code 扩展）",
        vendor: "GitHub",
        roots: &[
            RootSpec {
                platforms: MAC_ONLY,
                base: BaseDir::MacApplicationSupport,
                relative: "Code/User/globalStorage",
                env_override: None,
            },
            RootSpec {
                platforms: MAC_ONLY,
                base: BaseDir::MacApplicationSupport,
                relative: "Code - Insiders/User/globalStorage",
                env_override: None,
            },
            RootSpec {
                platforms: MAC_ONLY,
                base: BaseDir::MacApplicationSupport,
                relative: "VSCodium/User/globalStorage",
                env_override: None,
            },
            RootSpec {
                platforms: LINUX_ONLY,
                base: BaseDir::XdgConfig,
                relative: "Code/User/globalStorage",
                env_override: None,
            },
            RootSpec {
                platforms: LINUX_ONLY,
                base: BaseDir::XdgConfig,
                relative: "Code - Insiders/User/globalStorage",
                env_override: None,
            },
            RootSpec {
                platforms: LINUX_ONLY,
                base: BaseDir::XdgConfig,
                relative: "VSCodium/User/globalStorage",
                env_override: None,
            },
            RootSpec {
                platforms: WINDOWS_ONLY,
                base: BaseDir::AppDataRoaming,
                relative: "Code/User/globalStorage",
                env_override: None,
            },
            RootSpec {
                platforms: WINDOWS_ONLY,
                base: BaseDir::AppDataRoaming,
                relative: "Code - Insiders/User/globalStorage",
                env_override: None,
            },
            RootSpec {
                platforms: WINDOWS_ONLY,
                base: BaseDir::AppDataRoaming,
                relative: "VSCodium/User/globalStorage",
                env_override: None,
            },
        ],
        categories: &[
            CategorySpec {
                category: AiCacheCategory::Cache,
                paths: &["github.copilot"],
            },
            CategorySpec {
                category: AiCacheCategory::Sessions,
                paths: &["github.copilot-chat"],
            },
        ],
        protected: &["state.vscdb", "storage.json"],
    },
    // ── 6. GitHub Copilot CLI ──
    ToolSpec {
        id: "copilot-cli",
        name: "GitHub Copilot CLI",
        vendor: "GitHub",
        roots: &[RootSpec {
            platforms: ALL_PLATFORMS,
            base: BaseDir::Home,
            relative: ".copilot",
            env_override: None,
        }],
        categories: &[
            CategorySpec {
                category: AiCacheCategory::Cache,
                paths: &["pkg", "cache"],
            },
            CategorySpec {
                category: AiCacheCategory::Logs,
                paths: &["logs"],
            },
            CategorySpec {
                category: AiCacheCategory::Sessions,
                paths: &["session-state", "history", "history-session-state"],
            },
        ],
        protected: &["config.json", "mcp-config.json"],
    },
    // ── 7. Windsurf ──
    ToolSpec {
        id: "windsurf",
        name: "Windsurf",
        vendor: "Codeium",
        roots: &[
            RootSpec {
                platforms: MAC_ONLY,
                base: BaseDir::MacApplicationSupport,
                relative: "Windsurf",
                env_override: None,
            },
            RootSpec {
                platforms: LINUX_ONLY,
                base: BaseDir::XdgConfig,
                relative: "Windsurf",
                env_override: None,
            },
            RootSpec {
                platforms: WINDOWS_ONLY,
                base: BaseDir::AppDataRoaming,
                relative: "Windsurf",
                env_override: None,
            },
        ],
        categories: &[
            CategorySpec {
                category: AiCacheCategory::Cache,
                paths: &[
                    "Cache",
                    "Code Cache",
                    "CachedData",
                    "GPUCache",
                    "DawnGraphiteCache",
                    "DawnWebGPUCache",
                    "GrShaderCache",
                    "ShaderCache",
                    "CachedExtensionVSIXs",
                    "CachedProfilesData",
                    "Service Worker/CacheStorage",
                    "Service Worker/ScriptCache",
                ],
            },
            CategorySpec {
                category: AiCacheCategory::Logs,
                paths: &["Crashpad", "logs"],
            },
            CategorySpec {
                category: AiCacheCategory::Sessions,
                paths: &["User/workspaceStorage"],
            },
        ],
        protected: &["User/globalStorage", "User/settings.json", "extensions"],
    },
    // ── 8. Gemini CLI（v1 无 A 类白名单，只检测 + 展示） ──
    ToolSpec {
        id: "gemini-cli",
        name: "Gemini CLI",
        vendor: "Google",
        roots: &[RootSpec {
            platforms: ALL_PLATFORMS,
            base: BaseDir::Home,
            relative: ".gemini",
            env_override: Some("GEMINI_CLI_HOME"),
        }],
        categories: &[CategorySpec {
            category: AiCacheCategory::Sessions,
            paths: &["tmp", "history", "shell_history"],
        }],
        protected: &[
            "settings.json",
            "oauth_creds.json",
            "google_accounts.json",
            "trustedFolders.json",
            "projects.json",
            "policies",
            "extensions",
            "skills",
            "GEMINI.md",
            "installation_id",
            "user_id",
        ],
    },
    // ── 9. Ollama（model_cache 仅展示，不提供清理） ──
    ToolSpec {
        id: "ollama",
        name: "Ollama",
        vendor: "Ollama",
        roots: &[RootSpec {
            platforms: ALL_PLATFORMS,
            base: BaseDir::Home,
            relative: ".ollama",
            env_override: None,
        }],
        categories: &[
            CategorySpec {
                category: AiCacheCategory::Logs,
                paths: &["logs"],
            },
            CategorySpec {
                category: AiCacheCategory::ModelCache,
                paths: &["models"],
            },
        ],
        protected: &["id_ed25519", "id_ed25519.pub", "history"],
    },
];
