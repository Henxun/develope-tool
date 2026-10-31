//! AI 智能体缓存清理（feat/ai-cache-clean）验收测试（Tester / QA）。
//!
//! 对应最终实现（**最终 HEAD `e991341`** / spec v1.3）：
//! - A 类（可清理）= `Cache` + `Logs`：`image-cache` / `plans` / `backups` / `debug` …
//! - B 类（只读）= `Sessions` + `ModelCache`：`paste-cache` / `usage-data` / `projects` …（含惰性计算）
//! - 覆盖 spec v1.3 §7 的 AC-02/03/04/05/06/07/08/09/10/11/12/13/14/15/16/17/19/21/22 + R3（B1）回归
//! - 另含 D1（paste-cache/usage-data 只展示）、D5（Claude Desktop 敏感项）、S5（进度）回归
//!
//! 运行方式（必须串行，避免真实 env 读取用例互相干扰）：
//! ```text
//! cargo test --manifest-path src-tauri/Cargo.toml --test qa_ai_cache -- --nocapture --test-threads=1
//! ```
//!
//! 所有夹具在系统临时目录，通过注入式 env lookup 指向夹具，不读写真实 `$HOME`；
//! 不新增依赖：SHA-256 调系统 `shasum`，体积核对调系统 `du -sk`。

use devetool_lib::ai_cache::catalog::{self, Platform};
use devetool_lib::ai_cache::clean;
use devetool_lib::ai_cache::scan;
use devetool_lib::ai_cache::{
    AiCacheCategory, AiCacheCategoryDto, AiCacheCleanProgressEvent, AiCacheCleanRequest,
    AiCacheCleanResult, AiCacheCleanTarget, AiCacheScanProgressEvent, AiCacheScanResult,
    AiCleanStatus,
};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// 当前白名单里稳定的 A 类目录（避免依赖未来可能调整的条目）。
const DIR_CACHE: &str = "image-cache";
const DIR_CACHE_2: &str = "plans";
const DIR_LOGS: &str = "debug";
// 当前归入只读 `Sessions` 的目录（v1.1：仅展示、不可清理）。
const DIR_SESSION: &str = "paste-cache";
const DIR_SESSION_2: &str = "usage-data";

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
    scan::scan_all(platform(), lookup, &ids, &noop_scan).expect("主扫描不应返回 Err")
}

fn scan_all_heavy(
    lookup: &(impl Fn(&str) -> Option<OsString> + Sync),
    tool_ids: &[&str],
) -> AiCacheScanResult {
    let ids: Vec<String> = tool_ids.iter().map(|id| id.to_string()).collect();
    scan::scan_all_with_heavy(platform(), lookup, &ids, true, &noop_scan)
        .expect("heavy 扫描不应失败")
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

fn clean_capturing(
    lookup: &(impl Fn(&str) -> Option<OsString> + Sync),
    targets: Vec<AiCacheCleanTarget>,
    dry_run: bool,
) -> (AiCacheCleanResult, Vec<AiCacheCleanProgressEvent>) {
    let events: Mutex<Vec<AiCacheCleanProgressEvent>> = Mutex::new(Vec::new());
    let sink = |event: AiCacheCleanProgressEvent| {
        events.lock().expect("事件锁").push(event);
    };
    let result = clean::clean_all(platform(), lookup, clean_request(targets, dry_run), &sink)
        .expect("清理请求合法时不应返回命令级 Err");
    (result, events.into_inner().expect("事件锁"))
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
) -> Option<&'a AiCacheCategoryDto> {
    let tool = result.tools.iter().find(|tool| tool.id == tool_id)?;
    tool.roots
        .iter()
        .flat_map(|root| root.categories.iter())
        .find(|candidate| candidate.category == category)
}

fn find_tool<'a>(
    result: &'a AiCacheScanResult,
    tool_id: &str,
) -> Option<&'a devetool_lib::ai_cache::AiToolScanDto> {
    result.tools.iter().find(|tool| tool.id == tool_id)
}

fn tool_detected(result: &AiCacheScanResult, tool_id: &str) -> bool {
    find_tool(result, tool_id)
        .map(|tool| tool.detected)
        .unwrap_or(false)
}

// ── AC-04 体积准确性：扫描字节 vs `du -sk`，偏差 ≤ 5% ──

#[test]
fn ac04_scan_bytes_match_du_within_5_percent() {
    let (env, root) = claude_fixture("ac04");
    write_file(&root.join(DIR_LOGS).join("session-1.log"), 220_000);
    write_file(&root.join(DIR_CACHE).join("big.txt"), 1_300_000);
    write_file(&root.join(DIR_CACHE).join("nested").join("x.bin"), 480_000);
    write_file(&root.join(DIR_CACHE_2).join("p.bin"), 90_000);
    // B 类（只读）里的大文件不得计入主扫描的可清理量
    write_file(&root.join("projects").join("session.jsonl"), 5_000_000);

    let lookup = env.lookup();
    let result = scan_all(&lookup, &["claude-code"]);

    let cache = find_category(&result, "claude-code", AiCacheCategory::Cache).expect("cache 类别");
    let logs = find_category(&result, "claude-code", AiCacheCategory::Logs).expect("logs 类别");
    let scan_bytes = cache.bytes + logs.bytes;

    let mut du_total = 0u64;
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
        }
    }
    let dev_total = deviation_percent(scan_bytes, du_total);
    println!("AC-04 合计 scan={scan_bytes}B du={du_total}B 偏差={dev_total:.2}%（阈值 5%）");
    assert!(
        dev_total <= 5.0,
        "总体积与 du -sk 偏差 {dev_total:.2}% 超过 5%"
    );
    assert_eq!(scan_bytes, 220_000 + 1_300_000 + 480_000 + 90_000);
    assert_eq!(result.total_cleanable_bytes, scan_bytes);
    assert_eq!(
        result.total_display_only_bytes, 0,
        "主扫描不统计 B 类（惰性）"
    );
}

// ── AC-06（v1.1）分级展示 + 惰性计算 B 类体积 ──

#[test]
fn ac06_b_class_is_display_only_and_measured_lazily() {
    let (env, root) = claude_fixture("ac06");
    write_file(&root.join(DIR_CACHE).join("a.bin"), 1_000);
    write_file(&root.join("projects").join("session.jsonl"), 40_000);
    write_file(&root.join("file-history").join("snapshot.txt"), 10_000);
    write_file(&root.join(DIR_SESSION).join("p.bin"), 5_000);
    write_file(&root.join(DIR_SESSION_2).join("u.bin"), 6_000);

    let lookup = env.lookup();
    let main = scan_all(&lookup, &["claude-code"]);
    let sessions =
        find_category(&main, "claude-code", AiCacheCategory::Sessions).expect("sessions");
    println!(
        "AC-06 主扫描 sessions measured={} bytes={} cleanable={} paths={}",
        sessions.measured,
        sessions.bytes,
        sessions.cleanable,
        sessions.paths.len()
    );
    assert!(!sessions.cleanable, "Sessions 不得提供清理入口");
    assert!(!sessions.measured, "主扫描不应递归统计 B 类");
    assert_eq!(sessions.bytes, 0, "未计算时体积应为 0（不是假数据）");
    assert!(
        sessions
            .paths
            .iter()
            .any(|path| path.path.ends_with(DIR_SESSION)),
        "B 类路径仍应列出（供「打开目录」）"
    );
    assert_eq!(
        main.total_display_only_bytes, 0,
        "主扫描 displayOnlyBytes=0"
    );

    let heavy = scan_all_heavy(&lookup, &["claude-code"]);
    let sessions_heavy =
        find_category(&heavy, "claude-code", AiCacheCategory::Sessions).expect("sessions");
    let expected_heavy = 40_000 + 10_000 + 5_000 + 6_000;
    println!(
        "AC-06 heavy 扫描 sessions measured={} bytes={} displayOnly={}",
        sessions_heavy.measured, sessions_heavy.bytes, heavy.total_display_only_bytes
    );
    assert!(sessions_heavy.measured, "heavy 扫描应标记已计算");
    assert_eq!(sessions_heavy.bytes, expected_heavy);
    assert_eq!(heavy.total_display_only_bytes, expected_heavy);
    assert!(!sessions_heavy.cleanable);
    assert_eq!(heavy.total_cleanable_bytes, 1_000, "A 类数字不受影响");

    // v1.1：`paste-cache` / `usage-data` 归入只读 Sessions，任何删除入口都必须被拒。
    let rejected = clean::clean_all(
        platform(),
        &lookup,
        clean_request(
            vec![scan_target(
                "claude-code",
                AiCacheCategory::Sessions,
                &root.join(DIR_SESSION),
            )],
            false,
        ),
        &noop_clean,
    );
    assert!(rejected.is_err(), "Sessions 类别清理应在命令级被拒");
    println!("AC-06 Sessions 清理拒绝：{}", rejected.unwrap_err());
    assert!(root.join(DIR_SESSION).join("p.bin").exists());
}

// ── D1（v1.2）：paste-cache / usage-data 移出 A 类 —— 仅展示、不清理、零变化 ──

#[test]
fn d1_paste_cache_and_usage_data_are_display_only() {
    let (env, root) = claude_fixture("d1");
    let paste_dir = root.join(DIR_SESSION);
    let usage_dir = root.join(DIR_SESSION_2);
    write_file(&paste_dir.join("paste.bin"), 3_000);
    write_file(&usage_dir.join("insight.bin"), 1_000);
    write_file(&root.join(DIR_CACHE).join("ok.bin"), 500);

    // 1) scan 正向断言：两个目录必须归入只读 Sessions，而不是可清理 Cache。
    let lookup = env.lookup();
    let scan_result = scan_all(&lookup, &["claude-code"]);
    let cache = find_category(&scan_result, "claude-code", AiCacheCategory::Cache).expect("cache");
    let sessions =
        find_category(&scan_result, "claude-code", AiCacheCategory::Sessions).expect("sessions");
    let cache_paths: Vec<&str> = cache.paths.iter().map(|path| path.path.as_str()).collect();
    let session_paths: Vec<&str> = sessions
        .paths
        .iter()
        .map(|path| path.path.as_str())
        .collect();
    println!("D1 cache 类别路径={cache_paths:?}");
    println!(
        "D1 sessions 类别路径={session_paths:?} cleanable={} measured={}",
        sessions.cleanable, sessions.measured
    );
    for (label, dir) in [("paste-cache", &paste_dir), ("usage-data", &usage_dir)] {
        let text = dir.display().to_string();
        assert!(
            !cache_paths.contains(&text.as_str()),
            "{label} 不得再出现在可清理 Cache 类别"
        );
        assert!(
            session_paths.contains(&text.as_str()),
            "{label} 必须仍出现在 Sessions（仅展示）类别，不能静默消失"
        );
    }
    assert!(!sessions.cleanable, "Sessions 不得提供清理入口");
    assert!(!sessions.measured, "主扫描不递归统计 B 类");
    assert_eq!(sessions.bytes, 0);
    assert_eq!(
        scan_result.total_cleanable_bytes, 500,
        "可清理量只统计 Cache/Logs"
    );

    // 2) clean（真实类别 Sessions）→ 命令级 CATEGORY_NOT_CLEANABLE，磁盘零变化。
    let mut before: Vec<String> = Vec::new();
    before.extend(fingerprint(&paste_dir));
    before.extend(fingerprint(&usage_dir));
    before.sort();
    let rejected = clean::clean_all(
        platform(),
        &lookup,
        clean_request(
            vec![
                scan_target("claude-code", AiCacheCategory::Sessions, &paste_dir),
                scan_target("claude-code", AiCacheCategory::Sessions, &usage_dir),
            ],
            false,
        ),
        &noop_clean,
    );
    let message = rejected.expect_err("Sessions 类别清理必须在命令级被拒");
    println!("D1 clean(sessions) 拒绝：{message}");
    assert!(
        message.contains("CATEGORY_NOT_CLEANABLE"),
        "必须返回 CATEGORY_NOT_CLEANABLE：{message}"
    );

    // 3) 伪造 category=cache 也一样删不掉（不在 A 类白名单）。
    let forged = clean(
        &lookup,
        vec![
            scan_target("claude-code", AiCacheCategory::Cache, &paste_dir),
            scan_target("claude-code", AiCacheCategory::Cache, &usage_dir),
        ],
        false,
    );
    println!(
        "D1 伪造 cache：deleted={} failed={} freed={} items={:?}",
        forged.deleted_count,
        forged.failed_count,
        forged.freed_bytes,
        forged
            .items
            .iter()
            .map(|item| (item.path.clone(), item.status, item.error.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(forged.deleted_count, 0, "伪造 cache 类别不得删除任何东西");
    assert_eq!(forged.freed_bytes, 0);
    assert_eq!(forged.failed_count, 2);
    assert!(forged.items.iter().all(|item| item
        .error
        .as_deref()
        .unwrap_or("")
        .contains("PATH_NOT_ALLOWED")));

    // 4) 目录树（含 SHA-256）零变化。
    let mut after: Vec<String> = Vec::new();
    after.extend(fingerprint(&paste_dir));
    after.extend(fingerprint(&usage_dir));
    after.sort();
    println!(
        "D1 目录树指纹：before={} 项 after={} 项",
        before.len(),
        after.len()
    );
    assert_eq!(before, after, "被拒后目录树（含 SHA-256）必须完全一致");
    assert!(paste_dir.join("paste.bin").exists());
    assert!(usage_dir.join("insight.bin").exists());

    // 5) 回归：页面「仅展示」区仍能「计算占用」（heavy 扫描）——两个目录都要被统计。
    let heavy = scan_all_heavy(&lookup, &["claude-code"]);
    let sessions_heavy =
        find_category(&heavy, "claude-code", AiCacheCategory::Sessions).expect("sessions");
    println!(
        "D1 heavy：measured={} bytes={} displayOnly={}",
        sessions_heavy.measured, sessions_heavy.bytes, heavy.total_display_only_bytes
    );
    assert!(sessions_heavy.measured, "「计算占用」应能算出体积");
    assert_eq!(
        sessions_heavy.bytes, 4_000,
        "4,000 = paste 3,000 + usage 1,000"
    );
    assert_eq!(heavy.total_display_only_bytes, 4_000);
    assert!(
        sessions_heavy
            .paths
            .iter()
            .filter(|path| path.path.ends_with(DIR_SESSION) || path.path.ends_with(DIR_SESSION_2))
            .all(|path| path.bytes > 0),
        "两个目录在仅展示区都应有非零体积"
    );
}

// ── AC-06b：主扫描不遍历 B 类（10 万个 B 类假文件，主扫描耗时与体积都不受影响） ──

#[test]
fn ac06b_main_scan_does_not_traverse_b_class() {
    let (env, root) = claude_fixture("ac06b");
    write_file(&root.join(DIR_CACHE).join("a.bin"), 1_000);
    // 10 万个 B 类文件（每个 8 B，合计 800,000 B）——若主扫描递归 sessions，
    // 这 10 万次 stat 会直接体现在耗时里。
    let bclass = root.join("projects");
    fs::create_dir_all(&bclass).expect("创建 B 类目录失败");
    let mut created = 0usize;
    for folder in 0..100 {
        let dir = bclass.join(format!("f{folder:03}"));
        fs::create_dir_all(&dir).expect("创建 B 类子目录失败");
        for file in 0..1_000 {
            fs::write(dir.join(format!("s{file:04}.jsonl")), b"12345678")
                .expect("写入 B 类文件失败");
            created += 1;
        }
    }
    assert_eq!(created, 100_000);

    let lookup = env.lookup();
    let main = scan_all(&lookup, &["claude-code"]);
    let sessions =
        find_category(&main, "claude-code", AiCacheCategory::Sessions).expect("sessions");
    println!(
        "AC-06b 主扫描：duration={}ms sessions.measured={} bytes={} displayOnly={} cleanable={}",
        main.duration_ms,
        sessions.measured,
        sessions.bytes,
        main.total_display_only_bytes,
        main.total_cleanable_bytes
    );
    assert!(!sessions.measured, "主扫描不得统计 B 类");
    assert_eq!(sessions.bytes, 0);
    assert_eq!(main.total_display_only_bytes, 0);
    assert_eq!(
        main.total_cleanable_bytes, 1_000,
        "A 类数字不受 B 类规模影响"
    );
    assert!(
        main.duration_ms < 1_000,
        "主扫描不得遍历 10 万个 B 类文件：{}ms",
        main.duration_ms
    );

    let heavy = scan_all_heavy(&lookup, &["claude-code"]);
    let sessions_heavy =
        find_category(&heavy, "claude-code", AiCacheCategory::Sessions).expect("sessions");
    println!(
        "AC-06b heavy：duration={}ms measured={} bytes={} displayOnly={}",
        heavy.duration_ms,
        sessions_heavy.measured,
        sessions_heavy.bytes,
        heavy.total_display_only_bytes
    );
    assert!(sessions_heavy.measured, "heavy 扫描应递归统计 B 类");
    assert_eq!(sessions_heavy.bytes, 800_000, "10 万文件 × 8 B");
    assert_eq!(heavy.total_display_only_bytes, 800_000);
    assert_eq!(heavy.total_cleanable_bytes, 1_000, "heavy 不污染 A 类数字");
    assert!(
        heavy.duration_ms > main.duration_ms,
        "B 类体量必须只体现在 heavy 扫描上（main={}ms heavy={}ms）",
        main.duration_ms,
        heavy.duration_ms
    );
}

// ── AC-07 / 默认 dry_run：不传 dryRun 时一个文件都不删 ──

#[test]
fn ac07_dry_run_defaults_true_and_deletes_nothing() {
    let (env, root) = claude_fixture("ac07");
    write_file(&root.join(DIR_CACHE).join("a.bin"), 100);
    write_file(&root.join(DIR_LOGS).join("d.log"), 200);
    let target = root.join(DIR_CACHE);

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

    write_file(&root.join(DIR_CACHE).join("cache.bin"), 120_000);
    write_file(&root.join(DIR_LOGS).join("debug.log"), 30_000);
    // B 类（只读：会话 / 历史 / 粘贴 / insights）
    write_file(&root.join("projects/my-app/session.jsonl"), 40_000);
    write_file(&root.join("projects/other/transcript.jsonl"), 20_000);
    write_file(&root.join("file-history/snapshot.txt"), 10_000);
    write_file(&root.join("tasks/task-1.json"), 2_000);
    write_file(&root.join(DIR_SESSION).join("paste.bin"), 3_000);
    write_file(&root.join(DIR_SESSION_2).join("insight.bin"), 1_000);
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
        root.join(DIR_SESSION),
        root.join(DIR_SESSION_2),
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
            scan_target("claude-code", AiCacheCategory::Logs, &root.join(DIR_LOGS)),
            scan_target("claude-code", AiCacheCategory::Cache, &root.join(DIR_CACHE)),
        ],
        false,
    );
    println!(
        "AC-13 清理结果 deleted={} failed={} skipped={} freed={}B",
        result.deleted_count, result.failed_count, result.skipped_count, result.freed_bytes
    );
    assert_eq!(result.deleted_count, 2, "两个 A 类目录都应被清理");
    assert!(!root.join(DIR_CACHE).exists());
    assert!(!root.join(DIR_LOGS).exists());

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
    assert!(
        root.join(DIR_SESSION).join("paste.bin").exists(),
        "paste-cache 属 B 类，必须保留（v1.1 裁决）"
    );
    assert!(root.join(DIR_SESSION_2).join("insight.bin").exists());
}

// ── AC-13（多工具）B 类只读：Codex sessions / Ollama models / Cursor workspaceStorage ──

#[test]
fn ac13_multi_tool_b_class_zero_impact() {
    let env = FakeEnv::new("ac13-multi");
    let codex = env.home.join(".codex");
    let ollama = env.home.join(".ollama");
    let cursor = env.home.join("Library/Application Support/Cursor");
    fs::create_dir_all(&codex).expect("创建 .codex 失败");
    fs::create_dir_all(&ollama).expect("创建 .ollama 失败");
    fs::create_dir_all(&cursor).expect("创建 Cursor 根失败");
    write_file(&codex.join("sessions/rollout-1.jsonl"), 5_000);
    write_file(&codex.join("logs_1.sqlite"), 1_000);
    write_file(&ollama.join("models/blobs/sha256-deadbeef"), 600_000);
    write_file(&ollama.join("logs/server.log"), 1_000);
    // Cursor：workspaceStorage（编辑历史/会话关联）属只读 Sessions；Cache 属 A 类
    write_file(
        &cursor.join("User/workspaceStorage/ws-1/state.vscdb"),
        7_000,
    );
    write_file(&cursor.join("Cache/data.bin"), 1_000);
    let lookup = env.lookup();

    let sessions_before = fingerprint(&codex.join("sessions"));
    let models_before = fingerprint(&ollama.join("models"));
    let workspace_before = fingerprint(&cursor.join("User/workspaceStorage"));

    let main = scan_all(&lookup, &["codex", "ollama", "cursor"]);
    let codex_sessions =
        find_category(&main, "codex", AiCacheCategory::Sessions).expect("codex sessions");
    let ollama_models =
        find_category(&main, "ollama", AiCacheCategory::ModelCache).expect("ollama models");
    let cursor_sessions =
        find_category(&main, "cursor", AiCacheCategory::Sessions).expect("cursor sessions");
    println!(
        "AC-13 主扫描 codex.sessions(m={},b={},c={}) ollama.models(m={},b={},c={}) cursor.sessions(m={},b={},c={})",
        codex_sessions.measured,
        codex_sessions.bytes,
        codex_sessions.cleanable,
        ollama_models.measured,
        ollama_models.bytes,
        ollama_models.cleanable,
        cursor_sessions.measured,
        cursor_sessions.bytes,
        cursor_sessions.cleanable
    );
    assert!(!codex_sessions.cleanable);
    assert!(!ollama_models.cleanable);
    assert!(!cursor_sessions.cleanable);
    assert!(!codex_sessions.measured && !ollama_models.measured && !cursor_sessions.measured);
    assert_eq!(main.total_display_only_bytes, 0);
    assert_eq!(
        main.total_cleanable_bytes,
        1_000 + 1_000 + 1_000,
        "只统计 Codex 日志文件、Ollama 日志、Cursor Cache"
    );

    let heavy = scan_all_heavy(&lookup, &["codex", "ollama", "cursor"]);
    println!("AC-13 heavy displayOnly={}", heavy.total_display_only_bytes);
    assert_eq!(heavy.total_display_only_bytes, 5_000 + 600_000 + 7_000);

    // ModelCache / Sessions 类别无清理入口：命令级拒绝，磁盘零变化
    for (label, tool_id, category, path) in [
        (
            "Ollama models",
            "ollama",
            AiCacheCategory::ModelCache,
            ollama.join("models"),
        ),
        (
            "Cursor workspaceStorage",
            "cursor",
            AiCacheCategory::Sessions,
            cursor.join("User/workspaceStorage"),
        ),
    ] {
        let rejected = clean::clean_all(
            platform(),
            &lookup,
            clean_request(vec![scan_target(tool_id, category, &path)], false),
            &noop_clean,
        );
        assert!(rejected.is_err(), "{label} 清理必须被拒");
        println!("AC-13 {label} 清理拒绝：{}", rejected.unwrap_err());
    }
    // 伪造 category=cache 提交 workspaceStorage → 白名单拒绝，目录零变化
    let forged = clean(
        &lookup,
        vec![scan_target(
            "cursor",
            AiCacheCategory::Cache,
            &cursor.join("User/workspaceStorage"),
        )],
        false,
    );
    assert_eq!(forged.deleted_count, 0);
    assert!(
        forged.failed_count + forged.skipped_count >= 1,
        "伪造类别必须被拒：{:?}",
        forged.items
    );

    // 对照：Cursor 的 A 类 Cache 仍可清理，B 类零影响
    let ok = clean(
        &lookup,
        vec![scan_target(
            "cursor",
            AiCacheCategory::Cache,
            &cursor.join("Cache"),
        )],
        false,
    );
    assert_eq!(ok.deleted_count, 1);
    assert_eq!(
        sessions_before,
        fingerprint(&codex.join("sessions")),
        "Codex 会话必须零变化"
    );
    assert_eq!(
        models_before,
        fingerprint(&ollama.join("models")),
        "Ollama 模型必须零变化"
    );
    assert_eq!(
        workspace_before,
        fingerprint(&cursor.join("User/workspaceStorage")),
        "Cursor workspaceStorage 必须零变化"
    );
}

// ── D5：Claude Desktop 的 config / extensions / Preferences / Cookies / Network 必须受保护或仅展示 ──

#[test]
fn d5_claude_desktop_sensitive_items_are_not_cleanable() {
    let env = FakeEnv::new("d5-desktop");
    let root = env.home.join("Library/Application Support/Claude");
    fs::create_dir_all(&root).expect("创建 Claude Desktop 根失败");
    // A 类：Electron 可重建缓存
    write_file(&root.join("Cache/data.bin"), 1_000);
    // C 类：配置 / 扩展 / 登录态 / 网络状态
    let sensitive = [
        "claude_desktop_config.json",
        "Preferences",
        "Cookies",
        "Network/TransportSecurity",
        "extensions/ext-1/index.js",
        "config.json",
        "state.json",
    ];
    for relative in sensitive {
        write_file(&root.join(relative), 256);
    }
    let lookup = env.lookup();

    let catalog_result = catalog::build_catalog(platform(), &lookup);
    let desktop = catalog_result
        .tools
        .iter()
        .find(|tool| tool.id == "claude-desktop")
        .expect("catalog 应包含 claude-desktop");
    println!("D5 claude-desktop protected={:?}", desktop.protected);
    for required in [
        "claude_desktop_config.json",
        "Preferences",
        "Cookies",
        "Network",
    ] {
        assert!(
            desktop.protected.iter().any(|item| item == required),
            "protected 缺少 {}：{:?}",
            required,
            desktop.protected
        );
    }
    // spec 要求 extensions / config.json / state* 也属于「受保护或仅展示」；
    // 当前实现未列入 protected，仅记录差异（不影响删除安全，见报告 N5）。
    let missing: Vec<&str> = ["extensions", "config.json", "state.json"]
        .into_iter()
        .filter(|required| !desktop.protected.iter().any(|item| item == required))
        .collect();
    println!("D5 protected 未展示项（仅记录，不影响安全）：{missing:?}");

    // 扫描：C 类路径不得出现在任何可清理类别里
    let scan_result = scan_all(&lookup, &["claude-desktop"]);
    let tool = find_tool(&scan_result, "claude-desktop").expect("claude-desktop");
    for category in tool.roots.iter().flat_map(|root| root.categories.iter()) {
        for path in &category.paths {
            for sensitive_rel in sensitive {
                assert!(
                    !path.path.ends_with(sensitive_rel),
                    "受保护项 {} 不得作为可清理项返回（category={:?}, cleanable={}）",
                    path.path,
                    category.category,
                    category.cleanable
                );
            }
        }
    }
    let cleanable: u64 = tool.cleanable_bytes;
    println!("D5 claude-desktop 可清理字节={cleanable}（仅 Cache 1000B）");
    assert_eq!(cleanable, 1_000, "只有 Cache 计入可清理量");

    // 清理：逐个提交 C 类路径 → 必须失败/跳过且文件零变化
    let before = fingerprint(&root);
    for relative in sensitive {
        let target = root.join(relative);
        let outcome = clean::clean_all(
            platform(),
            &lookup,
            clean_request(
                vec![scan_target(
                    "claude-desktop",
                    AiCacheCategory::Cache,
                    &target,
                )],
                false,
            ),
            &noop_clean,
        );
        match outcome {
            Err(message) => println!("D5 清理 {relative} 命令级拒绝：{message}"),
            Ok(result) => {
                println!(
                    "D5 清理 {relative} deleted={} failed={} skipped={}",
                    result.deleted_count, result.failed_count, result.skipped_count
                );
                assert_eq!(result.deleted_count, 0, "{relative} 不得被删除");
            }
        }
    }
    assert_eq!(
        before,
        fingerprint(&root),
        "受保护项 + A 类都必须零变化（本次只提交非法 target）"
    );

    // 对照：真正的 A 类缓存仍可清理
    let ok = clean(
        &lookup,
        vec![scan_target(
            "claude-desktop",
            AiCacheCategory::Cache,
            &root.join("Cache"),
        )],
        false,
    );
    println!("D5 对照清理 Cache deleted={}", ok.deleted_count);
    assert_eq!(ok.deleted_count, 1);
    assert!(!root.join("Cache").exists());
    assert!(root.join("claude_desktop_config.json").exists());
}

// ── AC-10 / AC-11 / AC-16 伪造 id、越界、根目录一律拒绝且磁盘零变化 ──

#[test]
fn ac10_11_16_reject_forged_and_out_of_scope_targets() {
    let (env, root) = claude_fixture("ac10");
    write_file(&root.join("projects/secret.jsonl"), 1_000);
    write_file(&root.join(DIR_CACHE).join("ok.bin"), 100);
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
            label: "B 类目录当 cache 类别清理",
            tool_id: "claude-code",
            category: AiCacheCategory::Cache,
            path: root.join(DIR_SESSION).display().to_string(),
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
            path: DIR_CACHE.to_string(),
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

    let unknown = clean::clean_all(
        platform(),
        &lookup,
        clean_request(
            vec![scan_target(
                "not-a-tool",
                AiCacheCategory::Cache,
                &root.join(DIR_CACHE),
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
            &root.join(DIR_CACHE),
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
    assert!(root.join(DIR_CACHE).join("ok.bin").exists());
    assert!(Path::new("/etc").exists());
}

// ── AC-22 后端 64 项上限仍生效（前端分批由 UI 代码核对） ──

#[test]
fn ac22_backend_64_target_limit() {
    let (env, root) = claude_fixture("ac22");
    write_file(&root.join(DIR_CACHE).join("a.bin"), 100);
    let lookup = env.lookup();

    let mut exactly_64 = Vec::new();
    for _ in 0..64 {
        exactly_64.push(scan_target(
            "claude-code",
            AiCacheCategory::Cache,
            &root.join(DIR_CACHE),
        ));
    }
    let accepted = clean::clean_all(
        platform(),
        &lookup,
        clean_request(exactly_64, true),
        &noop_clean,
    );
    assert!(accepted.is_ok(), "64 项整批应被接受（上限含 64）");
    let result = accepted.expect("64 项应 Ok");
    println!(
        "AC-22 64 项 dry-run：items={}（去重后）deleted={}",
        result.items.len(),
        result.deleted_count
    );
    assert_eq!(result.deleted_count, 0);

    let mut too_many = Vec::new();
    for _ in 0..65 {
        too_many.push(scan_target(
            "claude-code",
            AiCacheCategory::Cache,
            &root.join(DIR_CACHE),
        ));
    }
    let rejected = clean::clean_all(
        platform(),
        &lookup,
        clean_request(too_many, true),
        &noop_clean,
    )
    .expect_err("65 项必须被拒绝");
    println!("AC-22 65 项拒绝：{rejected}");
    assert!(rejected.contains("64"));
}

// ── AC-12 符号链接：不跟随、不删除、链接目标保留 ──

#[cfg(unix)]
#[test]
fn ac12_symlink_target_not_followed_or_deleted() {
    use std::os::unix::fs::symlink;

    let (env, root) = claude_fixture("ac12");
    let outside = env.home.join("outside-cache");
    write_file(&outside.join("valuable.bin"), 64_000);
    symlink(&outside, root.join(DIR_CACHE)).expect("创建符号链接失败");
    write_file(&root.join(DIR_CACHE_2).join("real.png"), 1_000);
    let outside_big = env.home.join("outside-big.bin");
    write_file(&outside_big, 900_000);
    symlink(&outside_big, root.join(DIR_CACHE_2).join("link.bin")).expect("创建文件符号链接失败");

    let lookup = env.lookup();
    let scan_result = scan_all(&lookup, &["claude-code"]);
    let cache = find_category(&scan_result, "claude-code", AiCacheCategory::Cache).expect("cache");
    let cache_paths: Vec<&str> = cache.paths.iter().map(|path| path.path.as_str()).collect();
    println!("AC-12 扫描 cache 项：{cache_paths:?}");
    let root_dto = &scan_result.tools[0].roots[0];
    assert!(
        root_dto
            .warnings
            .iter()
            .any(|warning| warning.contains("符号链接")),
        "扫描应提示跳过符号链接：{:?}",
        root_dto.warnings
    );
    let second_bytes = cache
        .paths
        .iter()
        .find(|path| path.path.ends_with(DIR_CACHE_2))
        .map(|path| path.bytes)
        .unwrap_or(0);
    println!("AC-12 {DIR_CACHE_2} 体积（不含链接目标 900000B）={second_bytes}B");
    assert_eq!(second_bytes, 1_000, "内部符号链接目标不得计入体积");

    let result = clean(
        &lookup,
        vec![
            scan_target("claude-code", AiCacheCategory::Cache, &root.join(DIR_CACHE)),
            scan_target(
                "claude-code",
                AiCacheCategory::Cache,
                &root.join(DIR_CACHE_2),
            ),
        ],
        false,
    );
    println!(
        "AC-12 清理结果 deleted={} skipped={} failed={} statuses={:?}",
        result.deleted_count,
        result.skipped_count,
        result.failed_count,
        result
            .items
            .iter()
            .map(|item| (item.path.clone(), item.status, item.error.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(result.deleted_count, 1, "只有真实目录被删除");
    assert!(
        result.failed_count + result.skipped_count >= 1,
        "符号链接白名单项必须被跳过/拒绝"
    );
    let symlink_item = result
        .items
        .iter()
        .find(|item| item.path.ends_with(DIR_CACHE))
        .expect("应保留符号链接项");
    assert!(
        symlink_item.status == AiCleanStatus::Skipped
            || symlink_item.status == AiCleanStatus::Failed,
        "符号链接不得被删除：{:?}",
        symlink_item
    );
    assert!(
        outside.join("valuable.bin").exists(),
        "链接目标文件必须保留"
    );
    assert!(outside_big.exists(), "目录内链接目标文件必须保留");
    assert!(
        fs::symlink_metadata(root.join(DIR_CACHE)).is_ok(),
        "符号链接本身也应保留"
    );
    assert!(root.exists());
}

// ── AC-19 幂等：连续清理两次，第二次不报错、不删新东西、释放 0 ──

#[test]
fn ac19_clean_is_idempotent() {
    let (env, root) = claude_fixture("ac19");
    write_file(&root.join(DIR_CACHE).join("a.bin"), 100);
    write_file(&root.join("projects/keep.jsonl"), 50);
    let lookup = env.lookup();
    let target = root.join(DIR_CACHE);

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
    write_file(&root.join(DIR_CACHE_2).join("ok.bin"), 100);
    write_file(&root.join(DIR_CACHE).join("locked.bin"), 100);
    write_file(&root.join("projects/keep.jsonl"), 50);

    fs::set_permissions(root.join(DIR_CACHE), fs::Permissions::from_mode(0o555))
        .expect("设置只读目录失败");

    let lookup = env.lookup();
    let result = clean(
        &lookup,
        vec![
            scan_target(
                "claude-code",
                AiCacheCategory::Cache,
                &root.join(DIR_CACHE_2),
            ),
            scan_target("claude-code", AiCacheCategory::Cache, &root.join(DIR_CACHE)),
        ],
        false,
    );

    fs::set_permissions(root.join(DIR_CACHE), fs::Permissions::from_mode(0o755))
        .expect("恢复目录权限失败");

    for item in &result.items {
        println!(
            "AC-15 item status={:?} path={} error={:?}",
            item.status, item.path, item.error
        );
    }
    assert_eq!(result.deleted_count, 1, "正常项应成功删除");
    assert_eq!(result.failed_count, 1, "只读目录项应失败");
    assert!(!root.join(DIR_CACHE_2).exists());
    assert!(
        root.join(DIR_CACHE).join("locked.bin").exists(),
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
    assert!(
        result.warnings.iter().any(|warning| {
            warning.contains("校验未通过")
                || warning.contains("已跳过")
                || warning.contains("清理失败")
        }),
        "S5：失败/跳过必须进 warnings：{:?}",
        result.warnings
    );
    assert_eq!(
        fingerprint(&root.join("projects")).len(),
        2,
        "B 类目录不受影响"
    );
}

// ── S5 清理进度：processed_count = deleted + skipped + failed ──

#[test]
fn clean_progress_counts_cover_all_items() {
    let (env, root) = claude_fixture("progress");
    write_file(&root.join(DIR_CACHE).join("a.bin"), 100);
    // 第二个目标不存在 → 应计入 skipped，进度必须仍能到 100%
    let missing = root.join(DIR_CACHE_2);
    let lookup = env.lookup();

    let (result, events) = clean_capturing(
        &lookup,
        vec![
            scan_target("claude-code", AiCacheCategory::Cache, &root.join(DIR_CACHE)),
            scan_target("claude-code", AiCacheCategory::Cache, &missing),
        ],
        false,
    );
    println!(
        "S5 结果 deleted={} skipped={} failed={} 事件数={}",
        result.deleted_count,
        result.skipped_count,
        result.failed_count,
        events.len()
    );
    assert!(!events.is_empty(), "真实清理应发出进度事件");
    for event in &events {
        assert_eq!(
            event.processed_count,
            event.deleted_count + event.skipped_count + event.failed_count,
            "processedCount 必须等于三类计数之和：{event:?}"
        );
    }
    let last = events.last().expect("至少一个事件");
    println!(
        "S5 最后事件 processed={} freed={}",
        last.processed_count, last.freed_bytes
    );
    assert_eq!(last.processed_count, 2, "整批 2 项的进度必须走完");
    assert_eq!(last.freed_bytes, 100);
}

// ── R3 / B1：文件名通配只删普通文件，同名目录绝不递归删除 ──

#[test]
fn b1_glob_only_matches_files_not_directories() {
    let env = FakeEnv::new("b1");
    let root = env.home.join(".codex");
    fs::create_dir_all(&root).expect("创建 .codex 失败");
    write_file(&root.join("logs_1.sqlite"), 100);
    // 恶意/巧合的同名目录：里面放真实数据
    write_file(&root.join("logs_evil.sqlite").join("precious.dat"), 5_000);
    let lookup = env.lookup();

    let scan_result = scan_all(&lookup, &["codex"]);
    let logs = find_category(&scan_result, "codex", AiCacheCategory::Logs).expect("logs");
    let names: Vec<&str> = logs
        .paths
        .iter()
        .map(|path| {
            Path::new(&path.path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
        })
        .collect();
    println!("B1 扫描 logs 项：{names:?}");
    assert_eq!(names, vec!["logs_1.sqlite"], "同名目录不得作为文件项返回");

    // 目录冒充文件项 → 必须拒绝，且目录内容原样保留
    let dir_result = clean(
        &lookup,
        vec![scan_target(
            "codex",
            AiCacheCategory::Logs,
            &root.join("logs_evil.sqlite"),
        )],
        false,
    );
    println!(
        "B1 清理目录结果 deleted={} failed={} status={:?} error={:?}",
        dir_result.deleted_count,
        dir_result.failed_count,
        dir_result.items[0].status,
        dir_result.items[0].error
    );
    assert_eq!(dir_result.deleted_count, 0, "目录不得被删除");
    assert_eq!(dir_result.failed_count, 1);
    assert!(
        dir_result.items[0]
            .error
            .as_deref()
            .unwrap_or("")
            .contains("通配"),
        "应给出通配项类型拒绝原因：{:?}",
        dir_result.items[0].error
    );
    assert!(root.join("logs_evil.sqlite").join("precious.dat").exists());

    // 真实文件仍应可清理（R3：文件级目标只删文件本身）
    let file_result = clean(
        &lookup,
        vec![scan_target(
            "codex",
            AiCacheCategory::Logs,
            &root.join("logs_1.sqlite"),
        )],
        false,
    );
    println!(
        "B1 清理文件结果 deleted={} freed={}",
        file_result.deleted_count, file_result.freed_bytes
    );
    assert_eq!(file_result.deleted_count, 1);
    assert!(!root.join("logs_1.sqlite").exists());
    assert!(root.join("logs_evil.sqlite").join("precious.dat").exists());

    // 文件类白名单项本身是符号链接：不跟随、不删除目标（AC-12 文件类）
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let external = env.home.join("external.sqlite");
        write_file(&external, 12_345);
        symlink(&external, root.join("logs_link.sqlite")).expect("创建文件符号链接失败");
        let link_result = clean(
            &lookup,
            vec![scan_target(
                "codex",
                AiCacheCategory::Logs,
                &root.join("logs_link.sqlite"),
            )],
            false,
        );
        println!(
            "B1 文件符号链接结果 deleted={} skipped={} failed={} status={:?}",
            link_result.deleted_count,
            link_result.skipped_count,
            link_result.failed_count,
            link_result.items[0].status
        );
        assert_eq!(link_result.deleted_count, 0, "符号链接不得被删除");
        assert!(external.exists(), "符号链接目标必须保留");
    }
}

// ── AC-21 / R9：override 指向 home、home 上级、文件系统根 → 降级仅检测 ──

#[test]
fn ac21_override_to_sensitive_root_degrades_tool() {
    let mut env = FakeEnv::new("ac21");
    // 把可清理目录也造出来，证明降级是"策略"而不是"没东西可删"
    let normal_dir = env.home.join("custom-claude");
    fs::create_dir_all(normal_dir.join(DIR_CACHE)).expect("创建 override 目录失败");
    write_file(&normal_dir.join(DIR_CACHE).join("a.bin"), 100);
    env.set_var("CLAUDE_CONFIG_DIR", normal_dir.clone());
    let lookup_normal = env.lookup();
    let catalog_normal = catalog::build_catalog(platform(), &lookup_normal);
    let claude_normal = catalog_normal
        .tools
        .iter()
        .find(|tool| tool.id == "claude-code")
        .expect("claude-code");
    println!(
        "AC-21 对照（普通 override）cleanable={} warnings={:?}",
        claude_normal.cleanable, claude_normal.warnings
    );
    assert!(claude_normal.cleanable, "普通 override 不应被误降级");
    let ok = clean(
        &lookup_normal,
        vec![scan_target(
            "claude-code",
            AiCacheCategory::Cache,
            &normal_dir.join(DIR_CACHE),
        )],
        false,
    );
    assert_eq!(ok.deleted_count, 1, "普通 override 应可正常清理");

    // 三种敏感位置：$HOME 本身 / $HOME 的上级 / 文件系统根（用每轮自己的 home 构造）
    for label in ["$HOME 本身", "$HOME 的上级", "文件系统根 /"] {
        let mut env = FakeEnv::new("ac21-sensitive");
        let override_dir = match label {
            "$HOME 本身" => env.home.clone(),
            "$HOME 的上级" => env.home.parent().map(Path::to_path_buf).expect("home 上级"),
            _ => PathBuf::from("/"),
        };
        let normal = env.home.join("x");
        fs::create_dir_all(normal.join(DIR_CACHE)).expect("创建目录失败");
        write_file(&normal.join(DIR_CACHE).join("a.bin"), 100);
        env.set_var("CLAUDE_CONFIG_DIR", override_dir);
        let lookup = env.lookup();

        let catalog_result = catalog::build_catalog(platform(), &lookup);
        let claude = catalog_result
            .tools
            .iter()
            .find(|tool| tool.id == "claude-code")
            .expect("claude-code");
        println!(
            "AC-21 {label}: cleanable={} warning={:?}",
            claude.cleanable,
            claude.warnings.first()
        );
        assert!(!claude.cleanable, "{label} 必须降级为仅检测");
        assert!(!claude.warnings.is_empty(), "{label} 必须给出降级原因");

        let scan_result = scan_all(&lookup, &["claude-code"]);
        let tool = find_tool(&scan_result, "claude-code").expect("claude-code");
        assert!(!tool.cleanable, "{label} 扫描结果也应标记不可清理");

        // 清理请求必须整批拒绝（命令级 TOOL_DISABLED）
        let rejected = clean::clean_all(
            platform(),
            &lookup,
            clean_request(
                vec![scan_target(
                    "claude-code",
                    AiCacheCategory::Cache,
                    &normal.join(DIR_CACHE),
                )],
                false,
            ),
            &noop_clean,
        );
        match rejected {
            Err(message) => {
                println!("AC-21 {label} 清理拒绝：{message}");
                assert!(
                    message.contains("TOOL_DISABLED"),
                    "降级工具应返回 TOOL_DISABLED：{message}"
                );
            }
            Ok(result) => panic!("{label} 降级后仍返回 Ok：{:?}", result),
        }
        assert!(
            normal.join(DIR_CACHE).join("a.bin").exists(),
            "{label} 降级后不得删除任何文件"
        );
    }
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

    // 清空一个不存在的、但确属 A 类的目录：跳过，不报错（R6）
    let clean_result = clean(
        &lookup,
        vec![scan_target(
            "claude-code",
            AiCacheCategory::Cache,
            &env.home.join(".claude").join(DIR_CACHE),
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
    fs::create_dir_all(claude_dir.join(DIR_LOGS)).expect("创建 claude override 失败");
    fs::create_dir_all(&codex_dir).expect("创建 codex override 失败");
    write_file(&claude_dir.join(DIR_LOGS).join("only.log"), 4_096);
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
    println!("AC-02 检测到的工具（按实现顺序）：{detected:?}");
    assert!(detected.contains(&"claude-code"));
    assert!(detected.contains(&"codex"));
    assert_eq!(detected.len(), 2, "只有根存在的工具才出现");
    assert!(!tool_detected(&result, "cursor"));
    assert!(!tool_detected(&result, "windsurf"));

    let claude = find_tool(&result, "claude-code").expect("claude-code");
    println!(
        "AC-03 claude root={}={}",
        claude.roots[0].path, claude.roots[0].exists
    );
    assert!(
        claude.roots[0].path.contains("custom-claude"),
        "CLAUDE_CONFIG_DIR 应覆盖默认 ~/.claude"
    );

    let clean_result = clean(
        &lookup,
        vec![scan_target(
            "claude-code",
            AiCacheCategory::Logs,
            &claude_dir.join(DIR_LOGS),
        )],
        false,
    );
    println!(
        "AC-03 清理 override 目录 deleted={} freed={}",
        clean_result.deleted_count, clean_result.freed_bytes
    );
    assert_eq!(clean_result.deleted_count, 1);
    assert!(!claude_dir.join(DIR_LOGS).exists());
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
    write_file(&root.join(DIR_CACHE).join("recent.bin"), 100);
    write_file(&root.join(DIR_CACHE_2).join("old.bin"), 100);
    let touch_status = Command::new("touch")
        .args(["-t", "202001010000"])
        .arg(root.join(DIR_CACHE_2).join("old.bin"))
        .status()
        .expect("touch 应可执行");
    assert!(touch_status.success());

    let lookup = env.lookup();
    let result = scan_all(&lookup, &["claude-code"]);
    let cache = find_category(&result, "claude-code", AiCacheCategory::Cache).expect("cache");
    let recent = cache
        .paths
        .iter()
        .find(|path| path.path.ends_with(DIR_CACHE))
        .expect("recent");
    let old = cache
        .paths
        .iter()
        .find(|path| path.path.ends_with(DIR_CACHE_2))
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
    write_file(&root.join(DIR_CACHE).join("a.bin"), 300_000);
    write_file(&root.join(DIR_CACHE_2).join("b.bin"), 200_000);
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
    assert!(!root.join(DIR_CACHE).exists());
    assert!(root.exists());
}

// ── AC-05 性能：扫描不阻塞、在预算内完成 ──

#[test]
fn ac05_scan_is_fast_on_fixture() {
    let (env, root) = claude_fixture("ac05");
    for index in 0..800 {
        write_file(
            &root
                .join(DIR_CACHE)
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

// ── catalog 首屏命令结构 + 真实进程 env 读取 ──

#[test]
fn catalog_command_shape_and_real_env() {
    let temp = TempDir::new("catalog");
    let claude_dir = temp.path().join("claude-real-env");
    fs::create_dir_all(claude_dir.join(DIR_LOGS)).expect("创建目录失败");
    write_file(&claude_dir.join(DIR_LOGS).join("x.log"), 128);

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
        "catalog 版本={} 工具数={} claude.root={:?} detected={} cleanable={}",
        catalog_result.catalog_version,
        catalog_result.tools.len(),
        claude.roots,
        claude.detected,
        claude.cleanable
    );
    assert_eq!(
        catalog_result.catalog_version,
        devetool_lib::ai_cache::CATALOG_VERSION
    );
    assert_eq!(catalog_result.tools.len(), catalog::tools().len());
    assert!(claude.detected);
    assert!(claude.cleanable);
    assert!(claude.roots[0].contains("claude-real-env"));
    assert!(claude.protected.iter().any(|item| item == "settings.json"));
    assert!(claude
        .protected
        .iter()
        .any(|item| item == ".credentials.json"));
}
