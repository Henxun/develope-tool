//! `ai_cache` 单元测试（plan §11）。
//!
//! 不新增依赖：临时目录用 `std::env::temp_dir()` + `uuid` 手写 `TempDir`，
//! 环境变量通过可注入的 lookup 假查找器提供，避免 `set_var` 污染并行测试。

use super::catalog::{self, Platform};
use super::clean::{self, ValidatedTarget};
use super::scan;
use super::{
    AiCacheCategory, AiCacheCategoryDto, AiCacheCleanProgressEvent, AiCacheCleanRequest,
    AiCacheCleanResult, AiCacheCleanTarget, AiCacheScanProgressEvent, AiCacheScanResult,
    AiCleanStatus, AiPathKind, AiRisk,
};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

// ── 测试夹具 ──

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "devtoolkit-ai-cache-test-{label}-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("创建临时测试目录失败");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// 假的进程环境：HOME / USERPROFILE / XDG / APPDATA 全部指向临时目录。
struct FakeEnv {
    _temp: TempDir,
    home: PathBuf,
    vars: Vec<(String, PathBuf)>,
}

impl FakeEnv {
    fn new(label: &str) -> Self {
        let temp = TempDir::new(label);
        let home = temp.path().join("home");
        fs::create_dir_all(&home).expect("创建临时 HOME 失败");
        let mut env = Self {
            _temp: temp,
            home: home.clone(),
            vars: Vec::new(),
        };
        let config = home.join(".config");
        let data = home.join(".local").join("share");
        let cache = home.join(".cache");
        let roaming = home.join("AppData").join("Roaming");
        let local = home.join("AppData").join("Local");
        env.set_var("HOME", home.clone());
        env.set_var("USERPROFILE", home);
        env.set_var("XDG_CONFIG_HOME", config);
        env.set_var("XDG_DATA_HOME", data);
        env.set_var("XDG_CACHE_HOME", cache);
        env.set_var("APPDATA", roaming);
        env.set_var("LOCALAPPDATA", local);
        env
    }

    fn set_var(&mut self, key: &str, value: PathBuf) {
        self.vars.retain(|(name, _)| name != key);
        self.vars.push((key.to_string(), value));
    }

    fn lookup(&self) -> impl Fn(&str) -> Option<OsString> + Sync + '_ {
        move |key| {
            self.vars
                .iter()
                .find(|(name, _)| name.as_str() == key)
                .map(|(_, value)| value.clone().into_os_string())
        }
    }
}

fn write_file(path: &Path, size: usize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("创建测试父目录失败");
    }
    fs::write(path, vec![b'x'; size]).expect("写入测试文件失败");
}

/// 造一个假的 `~/.claude` 根目录。
fn claude_fixture(label: &str) -> (FakeEnv, PathBuf) {
    let env = FakeEnv::new(label);
    let root = env.home.join(".claude");
    fs::create_dir_all(&root).expect("创建假 .claude 失败");
    (env, root)
}

fn scan_target(tool_id: &str, category: AiCacheCategory, path: &Path) -> AiCacheCleanTarget {
    AiCacheCleanTarget {
        tool_id: tool_id.to_string(),
        category,
        path: path.display().to_string(),
    }
}

fn clean_request(targets: Vec<AiCacheCleanTarget>, dry_run: bool) -> AiCacheCleanRequest {
    AiCacheCleanRequest { targets, dry_run }
}

fn noop_scan(_: AiCacheScanProgressEvent) {}
fn noop_clean(_: AiCacheCleanProgressEvent) {}

fn find_category<'a>(
    result: &'a AiCacheScanResult,
    tool_id: &str,
    category: AiCacheCategory,
) -> &'a AiCacheCategoryDto {
    let tool = result
        .tools
        .iter()
        .find(|tool| tool.id == tool_id)
        .expect("扫描结果应包含工具");
    tool.roots
        .iter()
        .flat_map(|root| root.categories.iter())
        .find(|candidate| candidate.category == category && !candidate.paths.is_empty())
        .expect("扫描结果应包含该类别的路径")
}

// ── 11.1 catalog / 静态安全断言 ──

#[test]
fn catalog_tool_ids_unique() {
    let mut ids = std::collections::HashSet::new();
    for tool in catalog::tools() {
        assert!(!tool.id.trim().is_empty(), "工具 id 不能为空");
        assert!(ids.insert(tool.id), "工具 id 重复：{}", tool.id);
    }
}

#[test]
fn catalog_categories_valid() {
    let known = [
        AiCacheCategory::Cache,
        AiCacheCategory::Logs,
        AiCacheCategory::Sessions,
        AiCacheCategory::ModelCache,
    ];
    for tool in catalog::tools() {
        assert!(!tool.categories.is_empty(), "{} 没有类别", tool.id);
        for category in tool.categories {
            assert!(known.contains(&category.category));
            assert!(
                !category.paths.is_empty(),
                "{} {:?} 没有白名单路径",
                tool.id,
                category.category
            );
            for path in category.paths {
                assert!(!path.is_empty(), "{} 存在空白名单路径", tool.id);
                assert!(
                    !path.starts_with('/'),
                    "{} 白名单出现绝对路径：{path}",
                    tool.id
                );
                assert!(!path.contains(".."), "{} 白名单出现越界段：{path}", tool.id);
                assert!(
                    !path.contains('\\'),
                    "{} 白名单应统一使用 / 分隔：{path}",
                    tool.id
                );
                let stars = path.matches('*').count();
                assert!(stars <= 1, "{} 路径最多一个通配符：{path}", tool.id);
                if stars == 1 {
                    let last_segment = path.rsplit('/').next().unwrap_or(path);
                    assert!(
                        last_segment.contains('*'),
                        "{} 通配符只允许出现在末段：{path}",
                        tool.id
                    );
                }
            }
        }
    }
}

#[test]
fn catalog_never_lists_protected_basenames() {
    for tool in catalog::tools() {
        for category in tool.categories {
            if !category.category.is_cleanable() {
                continue;
            }
            for path in category.paths {
                let name = path
                    .rsplit('/')
                    .next()
                    .unwrap_or(path)
                    .replace('*', "")
                    .to_lowercase();
                assert!(
                    !catalog::FORBIDDEN_BASENAMES.contains(&name.as_str()),
                    "{} 的可清理白名单包含凭据/配置文件：{path}",
                    tool.id
                );
                assert!(
                    !name.ends_with(".key") && !name.ends_with(".pem"),
                    "{} 的可清理白名单包含密钥文件：{path}",
                    tool.id
                );
            }
        }
    }
}

#[test]
fn catalog_cleanable_paths_do_not_nest_protected() {
    fn related(a: &str, b: &str) -> bool {
        a == b || b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/"))
    }
    for tool in catalog::tools() {
        for category in tool.categories {
            for path in category.paths {
                for protected in tool.protected {
                    assert!(
                        !related(path, protected),
                        "{} 的白名单路径 {path} 与受保护项 {protected} 重叠",
                        tool.id
                    );
                }
            }
        }
    }
}

#[test]
fn resolve_roots_are_absolute_and_unique() {
    let platform = catalog::current_platform();
    let lookup = |key: &str| std::env::var_os(key);
    for tool in catalog::tools() {
        let roots = catalog::resolve_roots_with(tool, platform, &lookup);
        assert!(!roots.is_empty(), "{} 在当前平台没有根目录", tool.id);
        let mut keys = std::collections::HashSet::new();
        for root in &roots {
            assert!(
                root.path.is_absolute(),
                "{} 根目录不是绝对路径：{:?}",
                tool.id,
                root.path
            );
            assert!(
                keys.insert(catalog::path_key(&root.path)),
                "{} 根目录重复：{:?}",
                tool.id,
                root.path
            );
        }
    }
}

#[test]
fn env_override_wins_for_claude_root() {
    let mut env = FakeEnv::new("env-override");
    let override_dir = env.home.join("custom-claude");
    fs::create_dir_all(&override_dir).expect("创建 override 目录失败");
    env.set_var("CLAUDE_CONFIG_DIR", override_dir.clone());
    let lookup = env.lookup();
    let spec = catalog::find_tool("claude-code").expect("catalog 应包含 claude-code");
    let roots = catalog::resolve_roots_with(spec, catalog::current_platform(), &lookup);
    assert_eq!(roots.len(), 1);
    assert!(catalog::path_eq(&roots[0].path, &override_dir));
    // 主目录内的普通 override 不受降级护栏影响。
    let policy = catalog::tool_policy(spec, catalog::current_platform(), &lookup);
    assert!(policy.cleanable, "主目录内的 override 应保持可清理");
    assert!(policy.warnings.is_empty());
}

// ── 11.1b 降级护栏（Chief 裁决 1：override → home / home 上级 / 文件系统根） ──

#[test]
fn catalog_override_to_home_degrades_tool() {
    let mut env = FakeEnv::new("policy-home");
    env.set_var("CLAUDE_CONFIG_DIR", env.home.clone());
    let lookup = env.lookup();
    let platform = catalog::current_platform();

    let built = catalog::build_catalog(platform, &lookup);
    let claude = built
        .tools
        .iter()
        .find(|tool| tool.id == "claude-code")
        .expect("catalog 应包含 claude-code");
    assert!(
        !claude.cleanable,
        "override 指向 home 时必须 cleanable=false"
    );
    assert!(
        claude
            .warnings
            .iter()
            .any(|warning| warning.contains("降级")),
        "catalog 必须带降级 warning：{:?}",
        claude.warnings
    );
    println!(
        "catalog claude-code cleanable={} warning={:?}",
        claude.cleanable,
        claude.warnings.first()
    );
    println!(
        "OVERRIDE_HOME_EVIDENCE tool=claude-code cleanable={} roots={:?} warnings={:?}",
        claude.cleanable, claude.roots, claude.warnings
    );

    // 其它工具（未受影响）仍可清理。
    let codex = built
        .tools
        .iter()
        .find(|tool| tool.id == "codex")
        .expect("catalog 应包含 codex");
    assert!(codex.cleanable, "无关工具不应被降级");
}

#[test]
fn catalog_override_to_home_ancestor_degrades_tool() {
    let mut env = FakeEnv::new("policy-ancestor");
    let ancestor = env
        .home
        .parent()
        .expect("临时 HOME 应有上级目录")
        .to_path_buf();
    env.set_var("CLAUDE_CONFIG_DIR", ancestor);
    let lookup = env.lookup();
    let built = catalog::build_catalog(catalog::current_platform(), &lookup);
    let claude = built
        .tools
        .iter()
        .find(|tool| tool.id == "claude-code")
        .expect("catalog 应包含 claude-code");
    assert!(!claude.cleanable, "override 指向 home 上级时必须降级");
    assert!(claude
        .warnings
        .iter()
        .any(|warning| warning.contains("上级")));
}

#[test]
fn policy_rejects_home_ancestor_and_allows_inside_home() {
    let temp = TempDir::new("policy-pure");
    let home = temp.path().join("home");
    fs::create_dir_all(&home).expect("创建临时 HOME 失败");

    let ancestor = catalog::tool_policy_for_roots(
        &[catalog::ResolvedRoot {
            path: temp.path().to_path_buf(),
            exists: true,
        }],
        Some(&home),
    );
    assert!(!ancestor.cleanable);
    assert!(ancestor
        .warnings
        .iter()
        .any(|warning| warning.contains("上级")));

    let inside = catalog::tool_policy_for_roots(
        &[catalog::ResolvedRoot {
            path: home.join(".claude"),
            exists: false,
        }],
        Some(&home),
    );
    assert!(inside.cleanable, "home 内的根目录应允许清理");
}

#[cfg(unix)]
#[test]
fn policy_rejects_filesystem_root() {
    let policy = catalog::tool_policy_for_roots(
        &[catalog::ResolvedRoot {
            path: PathBuf::from("/"),
            exists: true,
        }],
        None,
    );
    assert!(!policy.cleanable, "文件系统根必须降级");
    assert!(policy.warnings.iter().any(|warning| warning.contains("根")));
}

#[test]
fn scan_downgraded_tool_has_no_cleanable_paths() {
    let mut env = FakeEnv::new("scan-downgraded");
    env.set_var("CLAUDE_CONFIG_DIR", env.home.clone());
    fs::create_dir_all(env.home.join("backups")).expect("创建 home/backups 失败");
    write_file(&env.home.join("backups").join("old.zip"), 200);
    let lookup = env.lookup();
    let platform = catalog::current_platform();

    let lazy = scan::scan_all_with_heavy(
        platform,
        &lookup,
        &["claude-code".to_string()],
        false,
        &noop_scan,
    )
    .expect("扫描应成功");
    let tool = &lazy.tools[0];
    assert!(!tool.cleanable, "降级工具 cleanable 必须为 false");
    assert_eq!(tool.cleanable_bytes, 0);
    assert_eq!(lazy.total_cleanable_bytes, 0);
    for root in &tool.roots {
        for category in &root.categories {
            assert!(
                !category.cleanable,
                "降级工具的任何类别都不得可清理：{:?}",
                category.category
            );
        }
    }
    assert!(
        tool.warnings.iter().any(|warning| warning.contains("降级")),
        "扫描结果必须带降级提示：{:?}",
        tool.warnings
    );

    let heavy = scan::scan_all_with_heavy(
        platform,
        &lookup,
        &["claude-code".to_string()],
        true,
        &noop_scan,
    )
    .expect("扫描应成功");
    let tool = &heavy.tools[0];
    assert_eq!(tool.cleanable_bytes, 0);
    assert_eq!(heavy.total_cleanable_bytes, 0);
    assert!(
        tool.display_only_bytes >= 200,
        "降级工具的体积只能进展示类别"
    );
}

#[test]
fn clean_rejects_downgraded_tool_and_keeps_files() {
    let mut env = FakeEnv::new("clean-disabled");
    env.set_var("CLAUDE_CONFIG_DIR", env.home.clone());
    let backups = env.home.join("backups");
    fs::create_dir_all(&backups).expect("创建 home/backups 失败");
    write_file(&backups.join("old.zip"), 200);
    let lookup = env.lookup();

    let error = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target("claude-code", AiCacheCategory::Cache, &backups)],
            false,
        ),
        &noop_clean,
    )
    .expect_err("降级工具必须整批拒绝");
    assert!(
        error.contains("TOOL_DISABLED"),
        "错误码应为 TOOL_DISABLED：{error}"
    );
    assert!(backups.exists(), "降级工具的文件不得被删除");
    assert!(backups.join("old.zip").exists());
}

// ── 11.2 路径校验（拒绝用例） ──

#[test]
fn validate_rejects_empty_and_relative() {
    let (env, _root) = claude_fixture("validate-empty");
    let lookup = env.lookup();
    let platform = catalog::current_platform();
    assert!(
        clean::validate_target("claude-code", AiCacheCategory::Cache, "", platform, &lookup)
            .is_err()
    );
    assert!(clean::validate_target(
        "claude-code",
        AiCacheCategory::Cache,
        "relative/path",
        platform,
        &lookup
    )
    .is_err());
}

#[test]
fn validate_rejects_parent_traversal() {
    let (env, root) = claude_fixture("validate-traversal");
    fs::create_dir_all(root.join("paste-cache")).expect("创建夹具失败");
    let lookup = env.lookup();
    let traversal = root.join("..").join("secret");
    let result = clean::validate_target(
        "claude-code",
        AiCacheCategory::Cache,
        &traversal.display().to_string(),
        catalog::current_platform(),
        &lookup,
    );
    assert!(result.is_err());
}

#[test]
fn validate_rejects_outside_root() {
    let (env, _root) = claude_fixture("validate-outside");
    let outside = env.home.join("other-tool").join("cache");
    fs::create_dir_all(&outside).expect("创建外部目录失败");
    let lookup = env.lookup();
    let result = clean::validate_target(
        "claude-code",
        AiCacheCategory::Cache,
        &outside.display().to_string(),
        catalog::current_platform(),
        &lookup,
    );
    assert!(result.is_err());
}

#[test]
fn validate_rejects_root_itself() {
    let (env, root) = claude_fixture("validate-root");
    let lookup = env.lookup();
    let result = clean::validate_target(
        "claude-code",
        AiCacheCategory::Cache,
        &root.display().to_string(),
        catalog::current_platform(),
        &lookup,
    );
    assert!(result.is_err());
}

#[test]
fn validate_rejects_non_allowlisted_subdir() {
    let (env, root) = claude_fixture("validate-protected");
    write_file(&root.join("settings.json"), 32);
    let lookup = env.lookup();
    let result = clean::validate_target(
        "claude-code",
        AiCacheCategory::Cache,
        &root.join("settings.json").display().to_string(),
        catalog::current_platform(),
        &lookup,
    );
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn validate_rejects_symlink_target() {
    use std::os::unix::fs::symlink;

    let (env, root) = claude_fixture("validate-symlink");
    let outside = env.home.join("outside-cache");
    fs::create_dir_all(&outside).expect("创建外部缓存目录失败");
    write_file(&outside.join("data.bin"), 64);
    symlink(&outside, root.join("image-cache")).expect("创建符号链接失败");
    let lookup = env.lookup();
    let rejection = clean::validate_target(
        "claude-code",
        AiCacheCategory::Cache,
        &root.join("image-cache").display().to_string(),
        catalog::current_platform(),
        &lookup,
    )
    .expect_err("符号链接目标必须被拒绝");
    assert_eq!(rejection.status, AiCleanStatus::Skipped);
    assert!(rejection.error.contains("PATH_IS_SYMLINK"));
}

#[test]
fn validate_rejects_file_bypass() {
    let (env, root) = claude_fixture("validate-bypass");
    write_file(&root.join("history.jsonl"), 8);
    let lookup = env.lookup();
    let result = clean::validate_target(
        "claude-code",
        AiCacheCategory::Cache,
        &root.join("history.jsonl").display().to_string(),
        catalog::current_platform(),
        &lookup,
    );
    assert!(result.is_err());
}

// ── 11.3 扫描 ──

#[test]
fn scan_missing_root_is_not_error() {
    let env = FakeEnv::new("scan-missing");
    let lookup = env.lookup();
    let result = scan::scan_all(
        catalog::current_platform(),
        &lookup,
        &["claude-code".to_string()],
        &noop_scan,
    )
    .expect("根目录不存在不应报错");
    let tool = &result.tools[0];
    assert!(!tool.detected);
    assert_eq!(tool.cleanable_bytes, 0);
    assert_eq!(result.total_cleanable_bytes, 0);
}

#[test]
fn scan_sums_only_allowlisted_dirs() {
    let (env, root) = claude_fixture("scan-allowlist");
    write_file(&root.join("image-cache").join("a.bin"), 100);
    write_file(&root.join("settings.json"), 999);
    write_file(&root.join("random").join("x.bin"), 50);
    let lookup = env.lookup();
    let result = scan::scan_all(
        catalog::current_platform(),
        &lookup,
        &["claude-code".to_string()],
        &noop_scan,
    )
    .expect("扫描应成功");
    let tool = &result.tools[0];
    assert!(tool.detected);
    assert_eq!(tool.cleanable_bytes, 100);
    assert_eq!(result.total_cleanable_bytes, 100);
    let cache = find_category(&result, "claude-code", AiCacheCategory::Cache);
    assert_eq!(cache.bytes, 100);
    assert_eq!(cache.paths.iter().map(|path| path.bytes).sum::<u64>(), 100);
}

#[cfg(unix)]
#[test]
fn scan_skips_symlink_children() {
    use std::os::unix::fs::symlink;

    let (env, root) = claude_fixture("scan-symlink-children");
    write_file(&root.join("image-cache").join("real.bin"), 10);
    let outside = env.home.join("outside-large.bin");
    write_file(&outside, 5000);
    symlink(&outside, root.join("image-cache").join("link.bin")).expect("创建符号链接失败");
    let lookup = env.lookup();
    let result = scan::scan_all(
        catalog::current_platform(),
        &lookup,
        &["claude-code".to_string()],
        &noop_scan,
    )
    .expect("扫描应成功");
    let cache = find_category(&result, "claude-code", AiCacheCategory::Cache);
    assert_eq!(cache.bytes, 10);
}

#[test]
fn scan_expands_root_level_file_glob() {
    let env = FakeEnv::new("scan-glob");
    let root = env.home.join(".codex");
    fs::create_dir_all(&root).expect("创建 .codex 失败");
    write_file(&root.join("logs_1.sqlite"), 10);
    write_file(&root.join("logs_2.sqlite"), 20);
    write_file(&root.join("logs_3.txt"), 30);
    let lookup = env.lookup();
    let result = scan::scan_all(
        catalog::current_platform(),
        &lookup,
        &["codex".to_string()],
        &noop_scan,
    )
    .expect("扫描应成功");
    let logs = find_category(&result, "codex", AiCacheCategory::Logs);
    assert_eq!(logs.bytes, 30);
    assert_eq!(logs.paths.len(), 2);
    assert!(logs.paths.iter().all(|path| path.kind == AiPathKind::File));
}

#[test]
fn scan_lazy_heavy_categories_not_measured() {
    let (env, root) = claude_fixture("scan-lazy-heavy");
    write_file(&root.join("image-cache").join("a.bin"), 100);
    write_file(&root.join("projects").join("session.jsonl"), 500);
    let lookup = env.lookup();
    let platform = catalog::current_platform();

    // 主扫描：可清理类别照常统计，sessions / model_cache 只列路径不算体积。
    let lazy = scan::scan_all_with_heavy(
        platform,
        &lookup,
        &["claude-code".to_string()],
        false,
        &noop_scan,
    )
    .expect("主扫描应成功");
    let cache = find_category(&lazy, "claude-code", AiCacheCategory::Cache);
    assert!(cache.measured);
    assert_eq!(cache.bytes, 100);
    assert_eq!(lazy.total_cleanable_bytes, 100);
    assert_eq!(lazy.total_display_only_bytes, 0, "主扫描不得统计 B 类体积");
    let sessions = find_category(&lazy, "claude-code", AiCacheCategory::Sessions);
    assert!(!sessions.measured, "主扫描中 B 类应为未计算");
    assert_eq!(sessions.bytes, 0);
    assert_eq!(sessions.paths.len(), 1, "路径仍需列出，供「计算占用」");
    assert_eq!(sessions.paths[0].bytes, 0);

    // 二次请求：带 includeHeavy 才递归统计 B 类。
    let heavy = scan::scan_all_with_heavy(
        platform,
        &lookup,
        &["claude-code".to_string()],
        true,
        &noop_scan,
    )
    .expect("计算占用应成功");
    let sessions = find_category(&heavy, "claude-code", AiCacheCategory::Sessions);
    assert!(sessions.measured);
    assert_eq!(sessions.bytes, 500);
    assert_eq!(heavy.total_display_only_bytes, 500);
    assert_eq!(heavy.total_cleanable_bytes, 100);
}

#[test]
fn scan_glob_ignores_directory_matches() {
    let env = FakeEnv::new("scan-glob-dir");
    let root = env.home.join(".codex");
    fs::create_dir_all(root.join("logs_trap.sqlite")).expect("创建同名目录失败");
    write_file(&root.join("logs_trap.sqlite").join("payload.bin"), 64);
    write_file(&root.join("logs_ok.sqlite"), 10);
    let lookup = env.lookup();
    let result = scan::scan_all(
        catalog::current_platform(),
        &lookup,
        &["codex".to_string()],
        &noop_scan,
    )
    .expect("扫描应成功");
    let logs = find_category(&result, "codex", AiCacheCategory::Logs);
    assert_eq!(
        logs.paths.len(),
        1,
        "目录不能命中文件型 glob：{:?}",
        logs.paths
    );
    assert!(
        logs.paths
            .iter()
            .all(|path| !path.path.ends_with("logs_trap.sqlite")),
        "同名目录不得作为文件项返回"
    );
    assert_eq!(logs.bytes, 10);
}

#[test]
fn scan_aggregates_multiple_roots() {
    let env = FakeEnv::new("scan-multi-root");
    let lookup = env.lookup();
    let platform = catalog::current_platform();
    let spec = catalog::find_tool("copilot-vscode").expect("catalog 应包含 copilot-vscode");
    let roots = catalog::resolve_roots_with(spec, platform, &lookup);
    assert!(roots.len() >= 2, "VS Code 扩展应有多个根目录");

    let mut expected = 0u64;
    for (index, root) in roots.iter().enumerate() {
        fs::create_dir_all(&root.path).expect("创建 VS Code 根失败");
        let size = (index + 1) * 10;
        write_file(&root.path.join("github.copilot").join("data.bin"), size);
        expected += size as u64;
    }

    let result = scan::scan_all(
        platform,
        &lookup,
        &["copilot-vscode".to_string()],
        &noop_scan,
    )
    .expect("扫描应成功");
    let tool = &result.tools[0];
    assert_eq!(tool.roots.len(), roots.len());
    assert_eq!(tool.cleanable_bytes, expected);
}

#[test]
fn scan_result_serializes_camel_case() {
    let category = AiCacheCategoryDto {
        category: AiCacheCategory::ModelCache,
        label: "模型文件".to_string(),
        risk: AiRisk::Redownload,
        cleanable: false,
        measured: true,
        default_selected: false,
        description: String::new(),
        paths: Vec::new(),
        bytes: 0,
    };
    let value = serde_json::to_value(&category).expect("序列化类别失败");
    assert_eq!(value["category"], "model_cache");
    assert_eq!(value["defaultSelected"], false);
    assert_eq!(value["cleanable"], false);
    assert_eq!(value["measured"], true);

    let result = AiCacheScanResult {
        platform: "macos".to_string(),
        catalog_version: 1,
        scanned_at_secs: 0,
        duration_ms: 0,
        total_cleanable_bytes: 0,
        total_display_only_bytes: 0,
        tools: Vec::new(),
        warnings: Vec::new(),
    };
    let value = serde_json::to_value(&result).expect("序列化扫描结果失败");
    assert!(value.get("totalCleanableBytes").is_some());
    assert!(value.get("totalDisplayOnlyBytes").is_some());
    assert!(value.get("scannedAtSecs").is_some());
    assert!(value.get("catalogVersion").is_some());
}

// ── 11.4 清理 ──

#[test]
fn clean_dry_run_touches_nothing() {
    let (env, root) = claude_fixture("clean-dry-run");
    write_file(&root.join("image-cache").join("a.bin"), 100);
    let lookup = env.lookup();
    let target = root.join("image-cache");
    let result = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target("claude-code", AiCacheCategory::Cache, &target)],
            true,
        ),
        &noop_clean,
    )
    .expect("dry-run 应成功");

    assert!(result.dry_run);
    assert_eq!(result.freed_bytes, 100);
    assert_eq!(result.deleted_count, 0);
    assert_eq!(result.failed_count, 0);
    assert!(result
        .items
        .iter()
        .all(|item| item.status == AiCleanStatus::WouldDelete));
    assert!(target.join("a.bin").exists(), "dry-run 不得删除任何文件");
}

#[test]
fn clean_permanent_removes_only_selected() {
    let (env, root) = claude_fixture("clean-permanent");
    write_file(&root.join("image-cache").join("a.bin"), 100);
    write_file(&root.join("settings.json"), 999);
    write_file(&root.join("random").join("x.bin"), 50);
    let lookup = env.lookup();
    let target = root.join("image-cache");

    let result = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target("claude-code", AiCacheCategory::Cache, &target)],
            false,
        ),
        &noop_clean,
    )
    .expect("永久删除应成功");

    assert!(!result.dry_run);
    assert_eq!(result.freed_bytes, 100);
    assert_eq!(result.deleted_count, 1);
    assert!(!target.exists());
    assert!(root.join("settings.json").exists(), "配置不得被删除");
    assert!(
        root.join("random").join("x.bin").exists(),
        "白名单外目录不得被删除"
    );
}

#[test]
fn clean_is_idempotent() {
    let (env, root) = claude_fixture("clean-idempotent");
    write_file(&root.join("image-cache").join("a.bin"), 100);
    let lookup = env.lookup();
    let target = root.join("image-cache");

    let first = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target("claude-code", AiCacheCategory::Cache, &target)],
            false,
        ),
        &noop_clean,
    )
    .expect("第一次清理应成功");
    assert_eq!(first.freed_bytes, 100);

    let second = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target("claude-code", AiCacheCategory::Cache, &target)],
            false,
        ),
        &noop_clean,
    )
    .expect("第二次清理（幂等）应成功");
    assert_eq!(second.freed_bytes, 0);
    assert_eq!(second.deleted_count, 0);
    assert_eq!(second.skipped_count, 1);
    assert_eq!(second.items[0].status, AiCleanStatus::Skipped);
}

#[test]
fn clean_rejects_sessions_category() {
    let (env, root) = claude_fixture("clean-sessions");
    fs::create_dir_all(root.join("projects")).expect("创建 projects 失败");
    let lookup = env.lookup();
    let error = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target(
                "claude-code",
                AiCacheCategory::Sessions,
                &root.join("projects"),
            )],
            false,
        ),
        &noop_clean,
    )
    .expect_err("sessions 类别不可清理");
    assert!(error.contains("CATEGORY_NOT_CLEANABLE"));
    assert!(root.join("projects").exists());
}

/// D1：`paste-cache`（历史粘贴内容）与 `usage-data`（/insights 历史报告）是用户可见
/// 痕迹，v1 从 A 类移到仅展示；clean 命令必须返回 CATEGORY_NOT_CLEANABLE 且零删除。
#[test]
fn d1_paste_cache_and_usage_data_are_display_only() {
    let (env, root) = claude_fixture("d1-display-only");
    // 仍在 A 类的 image-cache 作对照，保证 Cache 类别非空。
    write_file(&root.join("image-cache").join("keep.bin"), 50);
    write_file(&root.join("paste-cache").join("pasted.txt"), 120);
    write_file(&root.join("usage-data").join("report.json"), 80);
    let lookup = env.lookup();
    let platform = catalog::current_platform();

    // 扫描：两者只出现在 Sessions（仅展示）类别，不计入可清理体积。
    let result = scan::scan_all(platform, &lookup, &["claude-code".to_string()], &noop_scan)
        .expect("扫描应成功");
    let cache = find_category(&result, "claude-code", AiCacheCategory::Cache);
    assert_eq!(cache.bytes, 50, "A 类只应统计 image-cache");
    assert!(
        cache.paths.iter().all(|path| {
            !path.path.ends_with("paste-cache") && !path.path.ends_with("usage-data")
        }),
        "D1 目标不得出现在可清理类别：{:?}",
        cache.paths
    );
    let sessions = find_category(&result, "claude-code", AiCacheCategory::Sessions);
    assert!(!sessions.cleanable, "Sessions 类别必须不可清理");
    for name in ["paste-cache", "usage-data"] {
        assert!(
            sessions.paths.iter().any(|path| path.path.ends_with(name)),
            "D1：{name} 应在仅展示类别出现"
        );
    }
    assert_eq!(result.total_cleanable_bytes, 50);

    // clean 命令：归到 Sessions 类别 → CATEGORY_NOT_CLEANABLE，目录零变化。
    for name in ["paste-cache", "usage-data"] {
        let dir = root.join(name);
        let error = clean::clean_all(
            platform,
            &lookup,
            clean_request(
                vec![scan_target("claude-code", AiCacheCategory::Sessions, &dir)],
                false,
            ),
            &noop_clean,
        )
        .expect_err("D1 目标不可清理");
        assert!(
            error.contains("CATEGORY_NOT_CLEANABLE"),
            "{name} 的错误码应为 CATEGORY_NOT_CLEANABLE：{error}"
        );
    }
    // 前端伪造 category=cache 也不能删：路径已不在 A 类白名单。
    let forged = clean::clean_all(
        platform,
        &lookup,
        clean_request(
            vec![scan_target(
                "claude-code",
                AiCacheCategory::Cache,
                &root.join("paste-cache"),
            )],
            false,
        ),
        &noop_clean,
    )
    .expect("请求合法，单条拒绝");
    assert_eq!(forged.deleted_count, 0);
    assert_eq!(forged.failed_count, 1);

    assert!(root.join("paste-cache").join("pasted.txt").exists());
    assert!(root.join("usage-data").join("report.json").exists());
    assert!(root.join("image-cache").join("keep.bin").exists());
}

#[test]
fn clean_rejects_unknown_tool() {
    let (env, root) = claude_fixture("clean-unknown-tool");
    let lookup = env.lookup();
    let error = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target(
                "not-a-tool",
                AiCacheCategory::Cache,
                &root.join("paste-cache"),
            )],
            true,
        ),
        &noop_clean,
    )
    .expect_err("未知工具必须报错");
    assert!(error.contains("TOOL_NOT_FOUND"));
}

#[test]
fn clean_reports_partial_failure() {
    let (env, root) = claude_fixture("clean-partial");
    write_file(&root.join("image-cache").join("a.bin"), 100);
    let lookup = env.lookup();
    let existing = root.join("image-cache");
    let missing = root.join("todos");
    let result = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![
                scan_target("claude-code", AiCacheCategory::Cache, &existing),
                scan_target("claude-code", AiCacheCategory::Cache, &missing),
            ],
            false,
        ),
        &noop_clean,
    )
    .expect("部分失败整体仍应 Ok");

    assert_eq!(result.deleted_count, 1);
    assert_eq!(result.skipped_count, 1);
    assert_eq!(result.failed_count, 0);
    assert!(!existing.exists());
}

#[test]
fn clean_rejects_tool_root_without_deleting() {
    let (env, root) = claude_fixture("clean-root");
    let lookup = env.lookup();
    let result = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target("claude-code", AiCacheCategory::Cache, &root)],
            false,
        ),
        &noop_clean,
    )
    .expect("请求本身合法，单条拒绝");
    assert_eq!(result.failed_count, 1);
    assert_eq!(result.deleted_count, 0);
    assert!(root.exists(), "工具根目录绝不能删除");
}

#[test]
fn clean_rejects_parent_traversal_without_deleting() {
    let (env, root) = claude_fixture("clean-traversal");
    let outside = env.home.join("secret");
    fs::create_dir_all(&outside).expect("创建外部目录失败");
    let lookup = env.lookup();
    let escape = root.join("..").join("secret");
    let result = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target("claude-code", AiCacheCategory::Cache, &escape)],
            false,
        ),
        &noop_clean,
    )
    .expect("请求本身合法，单条拒绝");
    assert_eq!(result.failed_count, 1);
    assert!(outside.exists(), "越界目标不得被删除");
}

#[cfg(unix)]
#[test]
fn clean_skips_symlinked_target() {
    use std::os::unix::fs::symlink;

    let (env, root) = claude_fixture("clean-symlink");
    let outside = env.home.join("outside-cache");
    fs::create_dir_all(&outside).expect("创建外部目录失败");
    write_file(&outside.join("data.bin"), 64);
    symlink(&outside, root.join("image-cache")).expect("创建符号链接失败");
    let lookup = env.lookup();
    let target = root.join("image-cache");

    let result = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target("claude-code", AiCacheCategory::Cache, &target)],
            false,
        ),
        &noop_clean,
    )
    .expect("跳过符号链接不应报错");
    assert_eq!(result.deleted_count, 0);
    assert_eq!(result.skipped_count, 1);
    assert!(outside.join("data.bin").exists(), "链接目标不得被删除");
    assert!(fs::symlink_metadata(&target).is_ok(), "链接本身保留");
}

#[test]
fn clean_dedupes_duplicate_targets() {
    let (env, root) = claude_fixture("clean-duplicates");
    write_file(&root.join("image-cache").join("a.bin"), 100);
    let lookup = env.lookup();
    let target = root.join("image-cache");
    let mut second = scan_target("claude-code", AiCacheCategory::Cache, &target);
    second.path = format!("{}/", second.path);

    let result = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![
                scan_target("claude-code", AiCacheCategory::Cache, &target),
                second,
            ],
            false,
        ),
        &noop_clean,
    )
    .expect("重复目标应去重");
    assert_eq!(result.deleted_count, 1);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.freed_bytes, 100);
}

#[test]
fn dedupe_targets_drops_duplicates_and_descendants() {
    let temp = TempDir::new("dedupe");
    let parent = temp.path().join("cache");
    let child = parent.join("nested");
    fs::create_dir_all(&child).expect("创建嵌套目录失败");

    let make = |path: PathBuf| ValidatedTarget {
        tool_id: "claude-code".to_string(),
        category: AiCacheCategory::Cache,
        display_path: path.display().to_string(),
        lexical_path: path.clone(),
        canonical_path: fs::canonicalize(&path).expect("canonicalize 失败"),
        expects_file: false,
    };

    let kept = clean::dedupe_targets(vec![
        make(child.clone()),
        make(parent.clone()),
        make(parent.clone()),
    ]);
    assert_eq!(kept.len(), 1);
    assert_eq!(
        kept[0].canonical_path,
        fs::canonicalize(&parent).expect("canonicalize 失败")
    );
}

#[test]
fn clean_rejects_directory_matching_glob_and_keeps_contents() {
    let env = FakeEnv::new("clean-glob-dir");
    let root = env.home.join(".codex");
    let trap = root.join("logs_trap.sqlite");
    fs::create_dir_all(&trap).expect("创建同名目录失败");
    write_file(&trap.join("payload.bin"), 64);
    let lookup = env.lookup();

    let result = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![scan_target("codex", AiCacheCategory::Logs, &trap)],
            false,
        ),
        &noop_clean,
    )
    .expect("请求合法，单条应被拒绝");
    assert_eq!(result.deleted_count, 0);
    assert_eq!(result.failed_count, 1);
    assert!(trap.join("payload.bin").exists(), "同名目录内容不得被删除");
    assert!(trap.exists(), "同名目录本身不得被删除");
}

#[test]
fn clean_progress_counts_skipped_and_failed_as_processed() {
    let (env, root) = claude_fixture("clean-progress");
    write_file(&root.join("image-cache").join("a.bin"), 100);
    let lookup = env.lookup();
    let existing = root.join("image-cache");
    let missing = root.join("todos");
    let events = std::sync::Mutex::new(Vec::new());

    let result = clean::clean_all(
        catalog::current_platform(),
        &lookup,
        clean_request(
            vec![
                scan_target("claude-code", AiCacheCategory::Cache, &existing),
                scan_target("claude-code", AiCacheCategory::Cache, &missing),
            ],
            false,
        ),
        &|event| {
            events
                .lock()
                .expect("进度锁")
                .push((event.processed_count, event.skipped_count));
        },
    )
    .expect("部分失败整体应 Ok");

    assert_eq!(result.deleted_count, 1);
    assert_eq!(result.skipped_count, 1);
    let events = events.into_inner().expect("进度锁");
    let last = events.last().expect("应至少有一个进度事件");
    assert_eq!(last.0, 2, "已处理项应等于 deleted + skipped");
    assert_eq!(last.1, 1);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("已跳过")),
        "删除阶段的跳过项要进 warnings：{:?}",
        result.warnings
    );
}

#[test]
fn clean_result_serializes_camel_case() {
    let result = AiCacheCleanResult {
        dry_run: true,
        freed_bytes: 1024,
        deleted_count: 0,
        skipped_count: 0,
        failed_count: 0,
        items: Vec::new(),
        warnings: Vec::new(),
    };
    let value = serde_json::to_value(&result).expect("序列化清理结果失败");
    assert_eq!(value["dryRun"], true);
    assert_eq!(value["freedBytes"], 1024);
    assert!(value.get("deletedCount").is_some());
    assert!(value.get("skippedCount").is_some());
    assert!(value.get("failedCount").is_some());
}

// 测试平台辅助：确保 `Platform` 在各平台代码路径上都被引用，避免导入告警。
#[test]
fn platform_names_are_stable() {
    assert_eq!(Platform::MacOs.as_str(), "macos");
    assert_eq!(Platform::Linux.as_str(), "linux");
    assert_eq!(Platform::Windows.as_str(), "windows");
}
