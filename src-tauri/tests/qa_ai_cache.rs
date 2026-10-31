//! AI 智能体缓存清理（feat/ai-cache-clean）验收测试（Tester / QA）。
//!
//! 目的：逐条覆盖 spec.md §7 验收标准中**可自动化**的部分，并在 stdout 打印证据
//! （字节数、du -sk 对比、SHA-256 指纹等），供 specs/ai-cache-clean/qa-report.md 引用。
//!
//! 运行方式（必须串行，避免 env 读取用例互相干扰）：
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml --test qa_ai_cache -- --nocapture --test-threads=1
//! ```
//!
//! 设计约束：
//! - 所有夹具都在系统临时目录内，通过**注入的 env lookup** 指向夹具，不碰真实 `$HOME`；
//! - 不新增依赖：临时目录手写、SHA-256 调用系统 `shasum`、du 调用系统 `du -sk`；
//! - 覆盖 AC-02/03/04/07/08/09/10/11/12/13/14/15/16/17/19，其余（UI/跨平台）见报告说明。

use devetool_lib::ai_cache::catalog::{self, Platform};
use devetool_lib::ai_cache::clean;
use devetool_lib::ai_cache::scan;
use devetool_lib::ai_cache::{
    AiCacheCategory, AiCacheCleanProgressEvent, AiCacheCleanRequest, AiCacheCleanResult,
    AiCacheCleanTarget, AiCacheScanProgressEvent, AiCacheScanResult, AiCleanStatus,
};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// ── 夹具 ──

static UNIQUE: AtomicU64 = AtomicU64::new(0);

fn unique_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let counter = UNIQUE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "devtoolkit-ai-cache-qa-{label}-{}-{nanos}-{counter}",
        std::process::id()
    ))
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let path = unique_dir(label);
        fs::create_dir_all(&path).expect("创建临时测试目录失败");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // best effort：测试里改动过权限的目录先恢复，避免残留。
        restore_permissions(&self.path);
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn restore_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::symlink_metadata(path) {
            if meta.is_dir() {
                let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
                if let Ok(entries) = fs::read_dir(path) {
                    for entry in entries.flatten() {
                        restore_permissions(&entry.path());
                    }
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// 假的进程环境：HOME / XDG / APPDATA 全部指向临时目录。
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
        env.set_var("HOME", home.clone());
        env.set_var("USERPROFILE", home.clone());
        env.set_var("XDG_CONFIG_HOME", home.join(".config"));
        env.set_var("XDG_DATA_HOME", home.join(".local/share"));
        env.set_var("XDG_CACHE_HOME", home.join(".cache"));
        env.set_var("APPDATA", home.join("AppData/Roaming"));
        env.set_var("LOCALAPPDATA", home.join("AppData/Local"));
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
    // 用非全零内容，避免文件系统压缩/稀疏文件影响体积断言。
    let data: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
    fs::write(path, data).expect("写入测试文件失败");
}

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

fn platform() -> Platform {
    catalog::current_platform()
}

fn scan_all(
    lookup: &(impl Fn(&str) -> Option<OsString> + Sync),
    tool_ids: &[&str],
) -> AiCacheScanResult {
    let ids: Vec<String> = tool_ids.iter().map(|id| id.to_string()).collect();
    scan::scan_all(platform(), lookup, &ids, &noop_scan).expect("扫描不应返回 Err")
}

fn clean(
    lookup: &(impl Fn(&str) -> Option<OsString> + Sync),
    targets: Vec<AiCacheCleanTarget>,
    dry_run: bool,
) -> AiCacheCleanResult {
    clean::clean_all(
        platform(),
        lookup,
        clean_request(targets, dry_run),
        &noop_clean,
    )
    .expect("清理请求合法时不应返回命令级 Err")
}

/// 目录/文件树指纹：路径 + 类型 + 字节数 + SHA-256（符号链接只记录链接本身）。
fn fingerprint(path: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_fingerprint(path, &mut out);
    out.sort();
    out
}

fn collect_fingerprint(path: &Path, out: &mut Vec<String>) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        out.push(format!("missing|{}", path.display()));
        return;
    };
    if meta.file_type().is_symlink() {
        let target = fs::read_link(path)
            .map(|value| value.display().to_string())
            .unwrap_or_default();
        out.push(format!("link|{}|{}", path.display(), target));
        return;
    }
    if meta.is_file() {
        out.push(format!(
            "file|{}|{}|{}",
            path.display(),
            meta.len(),
            sha256(path).unwrap_or_else(|| "SHA_UNAVAILABLE".to_string())
        ));
        return;
    }
    if meta.is_dir() {
        out.push(format!("dir|{}", path.display()));
        let Ok(entries) = fs::read_dir(path) else {
            out.push(format!("unreadable|{}", path.display()));
            return;
        };
        let mut children: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        children.sort();
        for child in children {
            collect_fingerprint(&child, out);
        }
        return;
    }
    out.push(format!("other|{}", path.display()));
}

fn sha256(path: &Path) -> Option<String> {
    let output = Command::new("shasum")
        .arg("-a")
        .arg("256")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .next()
        .map(|value| value.to_string())
}

/// 对单个路径执行 `du -sk`，返回字节数（du 的 1K 块口径）。
fn du_bytes(path: &Path) -> Option<u64> {
    let output = Command::new("du").arg("-sk").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let kib: u64 = text.split_whitespace().next()?.parse().ok()?;
    Some(kib * 1024)
}

fn deviation_percent(scan_bytes: u64, du_bytes: u64) -> f64 {
    if scan_bytes == 0 {
        return 0.0;
    }
    let delta = (scan_bytes as f64 - du_bytes as f64).abs();
    delta / scan_bytes as f64 * 100.0
}

fn find_category<'a>(
    result: &'a AiCacheScanResult,
    tool_id: &str,
    category: AiCacheCategory,
) -> Option<&'a devetool_lib::ai_cache::AiCacheCategoryDto> {
    let tool = result.tools.iter().find(|tool| tool.id == tool_id)?;
    tool.roots
        .iter()
        .flat_map(|root| root.categories.iter())
        .find(|candidate| candidate.category == category)
}

fn tool_detected(result: &AiCacheScanResult, tool_id: &str) -> bool {
    result
        .tools
        .iter()
        .find(|tool| tool.id == tool_id)
        .map(|tool| tool.detected)
        .unwrap_or(false)
}

// ── AC-04 体积准确性：扫描字节 vs `du -sk`，偏差 ≤ 5% ──

#[test]
fn ac04_scan_bytes_match_du_within_5_percent() {
    let (env, root) = claude_fixture("ac04");
    write_file(&root.join("debug/session-1.log"), 220_000);
    write_file(&root.join("paste-cache/big.txt"), 1_300_000);
    write_file(&root.join("paste-cache/nested/x.bin"), 480_000);
    write_file(&root.join("image-cache/img.png"), 90_000);
    // 白名单外的大文件不得计入
    write_file(&root.join("projects/session.jsonl"), 5_000_000);

    let lookup = env.lookup();
    let result = scan_all(&lookup, &["claude-code"]);

    let cache = find_category(&result, "claude-code", AiCacheCategory::Cache).expect("cache 类别");
    let logs = find_category(&result, "claude-code", AiCacheCategory::Logs).expect("logs 类别");
    let scan_bytes = cache.bytes + logs.bytes;

    let mut du_total = 0u64;
    let mut per_path = Vec::new();
    for category in [cache, logs] {
        for path in &category.paths {
            let path_buf = PathBuf::from(&path.path);
            let du = du_bytes(&path_buf).expect("du -sk 应可执行");
            let dev = deviation_percent(path.bytes, du);
            println!(
                "AC-04 明细 {} scan={}B du={}B 偏差={:.2}%",
                path.path, path.bytes, du, dev
            );
            assert!(
                dev <= 5.0,
                "单项目录 {} 与 du -sk 偏差 {:.2}% 超过 5%（scan={} du={}）",
                path.path,
                dev,
                path.bytes,
                du
            );
            du_total += du;
            per_path.push((path.bytes, du));
        }
    }
    let dev_total = deviation_percent(scan_bytes, du_total);
    println!("AC-04 合计 scan={scan_bytes}B du={du_total}B 偏差={dev_total:.2}%（阈值 5%）");
    assert!(
        dev_total <= 5.0,
        "总体积与 du -sk 偏差 {dev_total:.2}% 超过 5%"
    );
    // 白名单外 5MB 不得计入
    assert_eq!(scan_bytes, 220_000 + 1_300_000 + 480_000 + 90_000);
    assert_eq!(result.total_cleanable_bytes, scan_bytes);
}

// ── AC-07 / 默认 dry_run：不传 dryRun 时一个文件都不删 ──

#[test]
fn ac07_dry_run_defaults_true_and_deletes_nothing() {
    let (env, root) = claude_fixture("ac07");
    write_file(&root.join("paste-cache/a.bin"), 100);
    write_file(&root.join("debug/d.log"), 200);
    let target = root.join("paste-cache");

    let payload = serde_json::json!({
        "targets": [{ "toolId": "claude-code", "category": "cache", "path": target.display().to_string() }]
    });
    let request: AiCacheCleanRequest =
        serde_json::from_value(payload).expect("缺省 dryRun 的请求应能反序列化");
    println!("AC-07 缺省 dryRun 反序列化结果 dryRun={}", request.dry_run);
    assert!(request.dry_run, "不传 dryRun 时必须默认 dry-run");

    let before = fingerprint(&root);
    let lookup = env.lookup();
    let result =
        clean::clean_all(platform(), &lookup, request, &noop_clean).expect("dry-run 应成功");
    let after = fingerprint(&root);

    println!(
        "AC-07 dry_run={} freed={} deleted={} statuses={:?}",
        result.dry_run,
        result.freed_bytes,
        result.deleted_count,
        result
            .items
            .iter()
            .map(|item| item.status)
            .collect::<Vec<_>>()
    );
    assert!(result.dry_run);
    assert_eq!(result.deleted_count, 0);
    assert!(result.freed_bytes > 0, "dry-run 仍应给出预计释放量");
    assert_eq!(before, after, "dry-run 后目录树（含 SHA-256）必须完全一致");
    assert!(target.join("a.bin").exists());

    // 显式 dryRun:false 才会真正删除——证明默认值确实是保护而不是全局禁用。
    let explicit: AiCacheCleanRequest = serde_json::from_value(serde_json::json!({
        "targets": [{ "toolId": "claude-code", "category": "cache", "path": target.display().to_string() }],
        "dryRun": false
    }))
    .expect("显式 dryRun:false 应能反序列化");
    assert!(!explicit.dry_run);
    let real =
        clean::clean_all(platform(), &lookup, explicit, &noop_clean).expect("真实清理应成功");
    println!(
        "AC-07 显式 dryRun:false 结果 freed={} deleted={}",
        real.freed_bytes, real.deleted_count
    );
    assert_eq!(real.deleted_count, 1);
    assert!(!target.exists());
}

// ── AC-13 B/C 类零影响：清理前后文件数 + SHA-256 完全一致 ──

#[test]
fn ac13_bc_untouched_by_clean() {
    let (env, root) = claude_fixture("ac13");

    // A 类（应被清理）
    write_file(&root.join("paste-cache/cache.bin"), 120_000);
    write_file(&root.join("debug/debug.log"), 30_000);
    // B 类（会话 / 历史）
    write_file(&root.join("projects/my-app/session.jsonl"), 40_000);
    write_file(&root.join("projects/other/transcript.jsonl"), 20_000);
    write_file(&root.join("file-history/snapshot.txt"), 10_000);
    write_file(&root.join("tasks/task-1.json"), 2_000);
    // C 类（凭据 / 配置 / 白名单外目录）
    write_file(&root.join(".credentials.json"), 512);
    write_file(&root.join("settings.json"), 1_000);
    write_file(&root.join("plugins/installed_plugins.json"), 4_000);
    write_file(&root.join("agent-memory/memo.md"), 8_000);
    write_file(&root.join("unknown-dir/file.dat"), 16_000);

    let protected_paths: Vec<PathBuf> = vec![
        root.join("projects"),
        root.join("file-history"),
        root.join("tasks"),
        root.join(".credentials.json"),
        root.join("settings.json"),
        root.join("plugins"),
        root.join("agent-memory"),
        root.join("unknown-dir"),
    ];
    let before: Vec<String> = protected_paths
        .iter()
        .flat_map(|path| fingerprint(path))
        .collect();
    let file_count_before = before
        .iter()
        .filter(|entry| entry.starts_with("file|"))
        .count();
    let hash_count_before = before
        .iter()
        .filter(|entry| entry.starts_with("file|") && !entry.contains("SHA_UNAVAILABLE"))
        .count();

    let lookup = env.lookup();
    let result = clean(
        &lookup,
        vec![
            scan_target("claude-code", AiCacheCategory::Logs, &root.join("debug")),
            scan_target(
                "claude-code",
                AiCacheCategory::Cache,
                &root.join("paste-cache"),
            ),
        ],
        false,
    );
    println!(
        "AC-13 清理结果 deleted={} failed={} skipped={} freed={}B",
        result.deleted_count, result.failed_count, result.skipped_count, result.freed_bytes
    );
    assert_eq!(result.deleted_count, 2, "两个 A 类目录都应被清理");
    assert!(!root.join("paste-cache").exists());
    assert!(!root.join("debug").exists());

    let after: Vec<String> = protected_paths
        .iter()
        .flat_map(|path| fingerprint(path))
        .collect();
    let file_count_after = after
        .iter()
        .filter(|entry| entry.starts_with("file|"))
        .count();
    println!(
        "AC-13 B/C 类文件数 before={file_count_before} after={file_count_after} 含 SHA-256 的文件数={hash_count_before}"
    );
    assert!(
        file_count_before > 0 && hash_count_before == file_count_before,
        "B/C 类指纹必须全部带 SHA-256（shasum 可用）"
    );
    assert_eq!(
        before, after,
        "B/C 类目录/文件的数量与 SHA-256 必须完全一致"
    );
    assert!(root.exists(), "工具根目录必须保留");
}

// ── AC-10 / AC-11 / AC-16 伪造 id、越界、根目录一律拒绝且磁盘零变化 ──

#[test]
fn ac10_11_16_reject_forged_and_out_of_scope_targets() {
    let (env, root) = claude_fixture("ac10");
    write_file(&root.join("projects/secret.jsonl"), 1_000);
    write_file(&root.join("paste-cache/ok.bin"), 100);
    let outside = env.home.join("outside/secret.txt");
    write_file(&outside, 500);
    let lookup = env.lookup();

    let outside_dir_before = fingerprint(&env.home.join("outside"));
    let projects_before = fingerprint(&root.join("projects"));

    struct Case {
        label: &'static str,
        tool_id: &'static str,
        category: AiCacheCategory,
        path: String,
    }

    let cases = vec![
        Case {
            label: "伪造 cacheId/工具 id：claude.projects",
            tool_id: "claude.projects",
            category: AiCacheCategory::Cache,
            path: root.join("projects").display().to_string(),
        },
        Case {
            label: "会话目录当 cache 类别清理",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: root.join("projects").display().to_string(),
        },
        Case {
            label: "sessions 类别本身不可清理",
            tool_id: "claude-code",
            category: AiCacheCategory::Sessions,
            path: root.join("projects").display().to_string(),
        },
        Case {
            label: "../ 越界路径",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: root
                .join("..")
                .join("outside/secret.txt")
                .display()
                .to_string(),
        },
        Case {
            label: "绝对路径 /etc",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: "/etc".to_string(),
        },
        Case {
            label: "把工具根目录当目标",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: root.display().to_string(),
        },
        Case {
            label: "把用户主目录当目标",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: env.home.display().to_string(),
        },
        Case {
            label: "白名单外子目录",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: root.join("unknown-dir").display().to_string(),
        },
        Case {
            label: "凭据文件",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: root.join(".credentials.json").display().to_string(),
        },
        Case {
            label: "空字符串路径",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: String::new(),
        },
        Case {
            label: "相对路径",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: "paste-cache".to_string(),
        },
    ];

    for case in cases {
        let outcome = clean::clean_all(
            platform(),
            &lookup,
            clean_request(
                vec![scan_target(
                    case.tool_id,
                    case.category,
                    Path::new(&case.path),
                )],
                false,
            ),
            &noop_clean,
        );
        match outcome {
            // 未知工具 / 伪造 id / 不可清理类别属命令级拒绝（R1：白名单外一律默认拒绝）。
            Err(message) => {
                assert!(!message.trim().is_empty(), "拒绝必须给出可读原因");
                println!("AC-10 拒绝 {:<28} -> 命令级错误 {message}", case.label);
            }
            Ok(result) => {
                assert_eq!(
                    result.deleted_count, 0,
                    "{} 必须被拒绝，不得删除任何东西",
                    case.label
                );
                assert!(
                    result.failed_count + result.skipped_count >= 1,
                    "{} 必须返回失败/跳过明细",
                    case.label
                );
                assert!(result.items.iter().all(|item| item.error.is_some()));
                println!(
                    "AC-10 拒绝 {:<28} -> status={:?} error={}",
                    case.label,
                    result.items[0].status,
                    result.items[0].error.as_deref().unwrap_or("")
                );
            }
        }
    }

    // 命令级拒绝：未知工具 id / 空请求 / 超过 64 项
    let unknown = clean::clean_all(
        platform(),
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
    );
    assert!(unknown.is_err(), "未知工具 id 应返回命令级错误");
    println!("AC-10 未知工具错误：{}", unknown.unwrap_err());

    let empty = clean::clean_all(
        platform(),
        &lookup,
        clean_request(Vec::new(), true),
        &noop_clean,
    );
    assert!(empty.is_err(), "空请求应返回命令级错误");
    println!("AC-10 空请求错误：{}", empty.unwrap_err());

    let mut too_many = Vec::new();
    for _ in 0..65 {
        too_many.push(scan_target(
            "claude-code",
            AiCacheCategory::Cache,
            &root.join("paste-cache"),
        ));
    }
    let oversized = clean::clean_all(
        platform(),
        &lookup,
        clean_request(too_many, true),
        &noop_clean,
    );
    assert!(oversized.is_err(), "超过 64 项应被拒绝");
    println!("AC-10 超限错误：{}", oversized.unwrap_err());

    // 磁盘零变化
    assert_eq!(
        projects_before,
        fingerprint(&root.join("projects")),
        "projects 会话目录必须零变化"
    );
    assert_eq!(
        outside_dir_before,
        fingerprint(&env.home.join("outside")),
        "工具根外目录必须零变化"
    );
    assert!(root.exists());
    assert!(root.join("paste-cache/ok.bin").exists());
    assert!(Path::new("/etc").exists());
}

// ── AC-12 符号链接：不跟随、不删除、链接目标保留 ──

#[cfg(unix)]
#[test]
fn ac12_symlink_target_not_followed_or_deleted() {
    use std::os::unix::fs::symlink;

    let (env, root) = claude_fixture("ac12");
    let outside = env.home.join("outside-cache");
    write_file(&outside.join("valuable.bin"), 64_000);
    symlink(&outside, root.join("paste-cache")).expect("创建符号链接失败");
    // 目录内部的符号链接指向外部大文件：不得计入体积、不得跟随删除
    write_file(&root.join("image-cache/real.png"), 1_000);
    let outside_big = env.home.join("outside-big.bin");
    write_file(&outside_big, 900_000);
    symlink(&outside_big, root.join("image-cache/link.bin")).expect("创建文件符号链接失败");

    let lookup = env.lookup();
    let scan_result = scan_all(&lookup, &["claude-code"]);
    let cache = find_category(&scan_result, "claude-code", AiCacheCategory::Cache).expect("cache");
    let paste_entry = cache
        .paths
        .iter()
        .find(|path| path.path.ends_with("paste-cache"));
    assert!(
        paste_entry.is_none(),
        "指向外部的白名单符号链接不应作为可清理项返回"
    );
    let root_dto = &scan_result.tools[0].roots[0];
    println!(
        "AC-12 扫描 warning：{:?}",
        root_dto
            .warnings
            .iter()
            .filter(|warning| warning.contains("符号链接"))
            .collect::<Vec<_>>()
    );
    assert!(
        root_dto
            .warnings
            .iter()
            .any(|warning| warning.contains("符号链接")),
        "扫描应提示跳过符号链接"
    );
    let image_bytes = cache
        .paths
        .iter()
        .find(|path| path.path.ends_with("image-cache"))
        .map(|path| path.bytes)
        .unwrap_or(0);
    println!("AC-12 image-cache 体积（不含链接目标 900000B）={image_bytes}B");
    assert_eq!(image_bytes, 1_000, "内部符号链接目标不得计入体积");

    let result = clean(
        &lookup,
        vec![
            scan_target(
                "claude-code",
                AiCacheCategory::Cache,
                &root.join("paste-cache"),
            ),
            scan_target(
                "claude-code",
                AiCacheCategory::Cache,
                &root.join("image-cache"),
            ),
        ],
        false,
    );
    println!(
        "AC-12 清理结果 deleted={} skipped={} statuses={:?}",
        result.deleted_count,
        result.skipped_count,
        result
            .items
            .iter()
            .map(|item| (item.path.clone(), item.status, item.error.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(result.deleted_count, 1, "只有真实目录 image-cache 被删除");
    assert_eq!(result.skipped_count, 1, "符号链接白名单项被跳过");
    let symlink_item = result
        .items
        .iter()
        .find(|item| item.path.ends_with("paste-cache"))
        .expect("应保留符号链接项");
    assert_eq!(symlink_item.status, AiCleanStatus::Skipped);
    assert!(symlink_item
        .error
        .as_deref()
        .unwrap_or("")
        .contains("PATH_IS_SYMLINK"));
    assert!(
        outside.join("valuable.bin").exists(),
        "链接目标文件必须保留"
    );
    assert!(outside_big.exists(), "目录内链接目标文件必须保留");
    assert!(
        fs::symlink_metadata(root.join("paste-cache")).is_ok(),
        "符号链接本身也应保留"
    );
    assert!(root.exists());
}

// ── AC-19 幂等：连续清理两次，第二次不报错、不删新东西、释放 0 ──

#[test]
fn ac19_clean_is_idempotent() {
    let (env, root) = claude_fixture("ac19");
    write_file(&root.join("paste-cache/a.bin"), 100);
    write_file(&root.join("projects/keep.jsonl"), 50);
    let lookup = env.lookup();
    let target = root.join("paste-cache");

    let first = clean(
        &lookup,
        vec![scan_target("claude-code", AiCacheCategory::Cache, &target)],
        false,
    );
    println!(
        "AC-19 第一次 freed={} deleted={}",
        first.freed_bytes, first.deleted_count
    );
    assert_eq!(first.deleted_count, 1);
    assert_eq!(first.freed_bytes, 100);

    // 两次清理之间新增的 B 类文件不得被第二次清理波及
    write_file(&root.join("projects/new-session.jsonl"), 70);
    let keep_before = fingerprint(&root.join("projects"));

    let second = clean(
        &lookup,
        vec![scan_target("claude-code", AiCacheCategory::Cache, &target)],
        false,
    );
    println!(
        "AC-19 第二次 freed={} deleted={} skipped={} failed={} error={:?}",
        second.freed_bytes,
        second.deleted_count,
        second.skipped_count,
        second.failed_count,
        second.items[0].error
    );
    assert_eq!(second.freed_bytes, 0, "第二次应释放 0 B");
    assert_eq!(second.deleted_count, 0);
    assert_eq!(second.failed_count, 0, "缺失目标应视为跳过而非失败（R6）");
    assert_eq!(second.skipped_count, 1);
    assert_eq!(second.items[0].status, AiCleanStatus::Skipped);
    assert_eq!(
        keep_before,
        fingerprint(&root.join("projects")),
        "第二次清理不得删新东西"
    );
}

// ── AC-15 单项失败隔离：目标目录只读 → 该项失败，其它项成功 ──

#[cfg(unix)]
#[test]
fn ac15_partial_failure_is_isolated() {
    use std::os::unix::fs::PermissionsExt;

    let (env, root) = claude_fixture("ac15");
    write_file(&root.join("paste-cache/ok.bin"), 100);
    write_file(&root.join("image-cache/locked.bin"), 100);
    write_file(&root.join("projects/keep.jsonl"), 50);

    // Unix 上父目录只读（无写权限）会让删除子项失败；恢复放在 finally 语义里。
    fs::set_permissions(root.join("image-cache"), fs::Permissions::from_mode(0o555))
        .expect("设置只读目录失败");

    let lookup = env.lookup();
    let result = clean(
        &lookup,
        vec![
            scan_target(
                "claude-code",
                AiCacheCategory::Cache,
                &root.join("paste-cache"),
            ),
            scan_target(
                "claude-code",
                AiCacheCategory::Cache,
                &root.join("image-cache"),
            ),
        ],
        false,
    );

    fs::set_permissions(root.join("image-cache"), fs::Permissions::from_mode(0o755))
        .expect("恢复目录权限失败");

    for item in &result.items {
        println!(
            "AC-15 item status={:?} path={} error={:?}",
            item.status, item.path, item.error
        );
    }
    assert_eq!(result.deleted_count, 1, "正常项应成功删除");
    assert_eq!(result.failed_count, 1, "只读目录项应失败");
    assert!(!root.join("paste-cache").exists());
    assert!(
        root.join("image-cache/locked.bin").exists(),
        "失败项不得半删"
    );
    let failed = result
        .items
        .iter()
        .find(|item| item.status == AiCleanStatus::Failed)
        .expect("应有失败明细");
    assert!(
        failed
            .error
            .as_deref()
            .unwrap_or("")
            .contains("DELETE_FAILED"),
        "失败原因应带错误码：{:?}",
        failed.error
    );
    println!("AC-15 失败原因：{}", failed.error.as_deref().unwrap_or(""));
    assert_eq!(
        fingerprint(&root.join("projects")).len(),
        2,
        "B 类目录不受影响"
    );
}

// ── AC-17 空态：所有环境变量指向空目录时不崩溃、返回空结果 ──

#[test]
fn ac17_empty_state_does_not_crash() {
    let env = FakeEnv::new("ac17-empty");
    let lookup = env.lookup();
    let result = scan_all(&lookup, &[]);
    let detected: Vec<&str> = result
        .tools
        .iter()
        .filter(|tool| tool.detected)
        .map(|tool| tool.id.as_str())
        .collect();
    println!(
        "AC-17 空环境 tools={} detected={:?} cleanable={} displayOnly={}",
        result.tools.len(),
        detected,
        result.total_cleanable_bytes,
        result.total_display_only_bytes
    );
    assert_eq!(result.tools.len(), catalog::tools().len());
    assert!(detected.is_empty(), "临时 HOME 下不应检测到任何工具");
    assert_eq!(result.total_cleanable_bytes, 0);
    assert_eq!(result.total_display_only_bytes, 0);

    // 环境变量显式指向「存在但为空」的目录 → 工具被检测到，但无缓存项且不崩溃
    let mut env2 = FakeEnv::new("ac17-empty-dirs");
    let claude_dir = env2.home.join("empty-claude");
    let codex_dir = env2.home.join("empty-codex");
    let gemini_dir = env2.home.join("empty-gemini");
    for dir in [&claude_dir, &codex_dir, &gemini_dir] {
        fs::create_dir_all(dir).expect("创建空目录失败");
    }
    env2.set_var("CLAUDE_CONFIG_DIR", claude_dir);
    env2.set_var("CODEX_HOME", codex_dir);
    env2.set_var("GEMINI_CLI_HOME", gemini_dir);
    let lookup2 = env2.lookup();
    let result2 = scan_all(&lookup2, &["claude-code", "codex", "gemini-cli"]);
    let detected2: Vec<&str> = result2
        .tools
        .iter()
        .filter(|tool| tool.detected)
        .map(|tool| tool.id.as_str())
        .collect();
    println!(
        "AC-17 空目录环境 detected={:?} cleanable={}",
        detected2, result2.total_cleanable_bytes
    );
    assert_eq!(detected2.len(), 3, "三个 env 覆盖的工具都应被检测到");
    assert_eq!(result2.total_cleanable_bytes, 0);

    // 清空一个不存在的白名单目标：跳过，不报错（R6）
    let clean_result = clean(
        &lookup,
        vec![scan_target(
            "claude-code",
            AiCacheCategory::Cache,
            &env.home.join(".claude/paste-cache"),
        )],
        false,
    );
    println!(
        "AC-17 清理不存在目标 skipped={} failed={}",
        clean_result.skipped_count, clean_result.failed_count
    );
    assert_eq!(clean_result.skipped_count, 1);
    assert_eq!(clean_result.failed_count, 0);
}

// ── AC-02 / AC-03 自动检测集合 + 环境变量根切换 ──

#[test]
fn ac02_ac03_detection_and_env_override() {
    let mut env = FakeEnv::new("ac02");
    let claude_dir = env.home.join("custom-claude");
    let codex_dir = env.home.join("custom-codex");
    fs::create_dir_all(claude_dir.join("debug")).expect("创建 claude override 失败");
    fs::create_dir_all(&codex_dir).expect("创建 codex override 失败");
    write_file(&claude_dir.join("debug/only.log"), 4_096);
    env.set_var("CLAUDE_CONFIG_DIR", claude_dir.clone());
    env.set_var("CODEX_HOME", codex_dir.clone());

    let lookup = env.lookup();
    let result = scan_all(&lookup, &[]);
    let detected: Vec<&str> = result
        .tools
        .iter()
        .filter(|tool| tool.detected)
        .map(|tool| tool.id.as_str())
        .collect();
    println!("AC-02 检测到的工具（按实现顺序）：{:?}", detected);
    assert!(detected.contains(&"claude-code"));
    assert!(detected.contains(&"codex"));
    assert_eq!(detected.len(), 2, "只有根存在的工具才出现");
    assert!(!tool_detected(&result, "cursor"));
    assert!(!tool_detected(&result, "windsurf"));

    let claude = result
        .tools
        .iter()
        .find(|tool| tool.id == "claude-code")
        .expect("claude-code");
    println!(
        "AC-03 claude root={}={}",
        claude.roots[0].path, claude.roots[0].exists
    );
    assert!(
        claude.roots[0].path.contains("custom-claude"),
        "CLAUDE_CONFIG_DIR 应覆盖默认 ~/.claude"
    );

    // 清理只作用于覆盖目录里的白名单项
    let clean_result = clean(
        &lookup,
        vec![scan_target(
            "claude-code",
            AiCacheCategory::Logs,
            &claude_dir.join("debug"),
        )],
        false,
    );
    println!(
        "AC-03 清理 override 目录 deleted={} freed={}",
        clean_result.deleted_count, clean_result.freed_bytes
    );
    assert_eq!(clean_result.deleted_count, 1);
    assert!(!claude_dir.join("debug").exists());
    assert!(claude_dir.exists(), "override 根目录本身保留");
    assert!(
        !env.home.join(".claude").exists(),
        "默认路径不应被创建或修改"
    );
}

// ── AC-14 运行中保护（in_use 标记） ──

#[cfg(unix)]
#[test]
fn ac14_in_use_flag_from_mtime() {
    let (env, root) = claude_fixture("ac14");
    write_file(&root.join("paste-cache/recent.bin"), 100);
    write_file(&root.join("image-cache/old.bin"), 100);
    // 把 image-cache 的 mtime 改为 1 小时前（10 分钟窗口之外）
    let touch_status = Command::new("touch")
        .args(["-t", "202001010000"])
        .arg(root.join("image-cache/old.bin"))
        .status()
        .expect("touch 应可执行");
    assert!(touch_status.success());

    let lookup = env.lookup();
    let result = scan_all(&lookup, &["claude-code"]);
    let cache = find_category(&result, "claude-code", AiCacheCategory::Cache).expect("cache");
    let recent = cache
        .paths
        .iter()
        .find(|path| path.path.ends_with("paste-cache"))
        .expect("recent");
    let old = cache
        .paths
        .iter()
        .find(|path| path.path.ends_with("image-cache"))
        .expect("old");
    println!(
        "AC-14 recent.in_use={} (mtime={}) old.in_use={} (mtime={})",
        recent.in_use, recent.last_modified_ms, old.in_use, old.last_modified_ms
    );
    assert!(recent.in_use, "10 分钟内修改过 → in_use=true");
    assert!(!old.in_use, "超过 10 分钟 → in_use=false");
}

// ── AC-08 / AC-09 释放量 = 清理前字节数；清理后重扫归零 ──

#[test]
fn ac08_ac09_freed_bytes_and_rescan() {
    let (env, root) = claude_fixture("ac08");
    write_file(&root.join("paste-cache/a.bin"), 300_000);
    write_file(&root.join("image-cache/b.bin"), 200_000);
    let lookup = env.lookup();

    let before_scan = scan_all(&lookup, &["claude-code"]);
    let before_bytes = before_scan.total_cleanable_bytes;
    let targets: Vec<AiCacheCleanTarget> =
        find_category(&before_scan, "claude-code", AiCacheCategory::Cache)
            .expect("cache")
            .paths
            .iter()
            .map(|path| scan_target("claude-code", AiCacheCategory::Cache, Path::new(&path.path)))
            .collect();

    let result = clean(&lookup, targets, false);
    println!(
        "AC-08 清理前 scan={}B freed={}B deleted={} 误差={:.2}%",
        before_bytes,
        result.freed_bytes,
        result.deleted_count,
        (before_bytes as f64 - result.freed_bytes as f64).abs() / before_bytes as f64 * 100.0
    );
    assert_eq!(result.deleted_count, 2);
    assert_eq!(result.freed_bytes, before_bytes);

    let after_scan = scan_all(&lookup, &["claude-code"]);
    println!(
        "AC-09 清理后 cleanable={}B（应归零）",
        after_scan.total_cleanable_bytes
    );
    assert_eq!(after_scan.total_cleanable_bytes, 0);
    assert!(!root.join("paste-cache").exists());
    assert!(root.exists());
}

// ── AC-05 性能：扫描不阻塞、在预算内完成 ──

#[test]
fn ac05_scan_is_fast_on_fixture() {
    let (env, root) = claude_fixture("ac05");
    // 约 100 MB / 800 文件
    for index in 0..800 {
        write_file(
            &root
                .join("paste-cache")
                .join(format!("part-{:03}", index / 100))
                .join(format!("chunk-{index:03}.bin")),
            128 * 1024,
        );
    }
    let lookup = env.lookup();
    let started = Instant::now();
    let result = scan_all(&lookup, &[]);
    let elapsed = started.elapsed();
    let bytes = result.total_cleanable_bytes;
    println!(
        "AC-05 全量扫描 {} 工具 用时={}ms 可清理={}B ({:.1} MB/s)",
        result.tools.len(),
        elapsed.as_millis(),
        bytes,
        bytes as f64 / 1024.0 / 1024.0 / elapsed.as_secs_f64().max(0.001)
    );
    assert_eq!(bytes, 800 * 128 * 1024);
    assert!(
        elapsed.as_millis() < 5_000,
        "≤5s 预算：实际 {}ms",
        elapsed.as_millis()
    );
}

// ── 补充：catalog 首屏命令的返回结构 + 环境变量真进程读取 ──

#[test]
fn catalog_command_shape_and_real_env() {
    let temp = TempDir::new("catalog");
    let claude_dir = temp.path().join("claude-real-env");
    fs::create_dir_all(claude_dir.join("debug")).expect("创建目录失败");
    write_file(&claude_dir.join("debug/x.log"), 128);

    // 真实进程环境变量（命令实现 mod.rs:237-239 env_lookup 读的就是它）
    std::env::set_var("CLAUDE_CONFIG_DIR", &claude_dir);
    let lookup = |key: &str| std::env::var_os(key);
    let catalog_result = catalog::build_catalog(platform(), &lookup);
    std::env::remove_var("CLAUDE_CONFIG_DIR");

    let claude = catalog_result
        .tools
        .iter()
        .find(|tool| tool.id == "claude-code")
        .expect("catalog 应包含 claude-code");
    println!(
        "catalog 版本={} 工具数={} claude.root={:?} detected={}",
        catalog_result.catalog_version,
        catalog_result.tools.len(),
        claude.roots,
        claude.detected
    );
    assert_eq!(
        catalog_result.catalog_version,
        devetool_lib::ai_cache::CATALOG_VERSION
    );
    assert_eq!(catalog_result.tools.len(), catalog::tools().len());
    assert!(claude.detected);
    assert!(claude.roots[0].contains("claude-real-env"));
    assert!(claude.protected.iter().any(|item| item == "settings.json"));
    assert!(claude
        .protected
        .iter()
        .any(|item| item == ".credentials.json"));
}
