# QA 验收报告 — AI 智能体缓存清理（v1）

- 执行人：Tester（测试）
- 时间：2026-10-31（Asia/Shanghai）
- 验收对象：`feat/ai-cache-clean`
  - **本轮回归 HEAD：`b8d6bf3`**（`fix: 降级工具禁用勾选并展示受保护项`）
  - 前序交付：`dd34992`（页面）、`c146111`（Rust 模块）、`27ae4dd`（模块可见性）、`139c818`（本测试与 v1 报告）
  - 本轮修复：`6d0af9f`（R9 降级 + B 类惰性计算）、`52b111e`（按需计算/64 分批/进度）、`1f52208`（B1 通配目录防护）、`b8d6bf3`（降级禁用 + 受保护项展示）
- 验收依据：
  - `specs/ai-cache-clean/spec.md` **v1.1（当前工作区版本，未提交；含 AC-21/22/23）**
  - `specs/ai-cache-clean/plan.md`（v1.1，工作区版本）
  - 已提交的 v1.0 作为对照
- 验收环境：macOS（Darwin，Apple Silicon）、rustc/cargo 1.99.1、Node v24.21.0
- 环境限制：本机**无真实 AI 工具目录**（全部用临时夹具 + 注入 env）；**无法运行 `npm run tauri:dev`**（需编译开窗 Tauri 应用），UI 只做构建 + 代码核对。

---

## 0. 结论速览

| 维度 | 结果 |
|---|---|
| AC-01…AC-23（23 条，v1.1 口径） | **通过 19 条**、**失败 1 条**（AC-23 系统缓存根未实现）、**无法完整验证 3 条**（AC-01 / AC-05 / AC-17，UI 或真实规模不可用） |
| 必测 8 项（Chief 指定） | **8/8 通过**（v1 时第 3 项按 spec v1.1 口径重新判定） |
| 自动化测试 | `qa_ai_cache` 集成 **19/19 通过**；既有单测 **47/47 通过**；`cargo test` 全绿 |
| 质量门 | `cargo fmt --check` ✅、`cargo clippy --all-targets -D warnings` ✅、`cargo test` ✅、`npm run lint` ✅、`npm run build` ✅（含 `/tools/ai-cache-clean`） |
| 相对 v1 报告 | 3 个中危问题已修复并回归（AC-06 惰性计算、B1 通配目录防护、AC-21 降级护栏）；D3/D5 由 spec v1.1 裁决关闭；D1 实现已按安全方向落地但 **spec 文本方向相反**；**AC-23（系统缓存根）未实现** |

**一句话结论**：后端安全硬规则、清理正确性、幂等、失败隔离、B/C 类零影响、惰性计算、R9 降级、64 分批（前端代码）均实测通过，**代码质量门全绿**；阻塞项只剩 **AC-23 未实现** 与 **spec v1.1 工作区文本与实现的两处方向冲突**，需要 Chief 先收敛规格基线再决定 PR 结论。

### 必测 8 项结果（回归后）

| # | 必测项 | 结论 | 关键证据（本轮） |
|---|---|---|---|
| 1 | 扫描体积 vs `du -sk` 偏差 ≤5% | ✅ 通过 | 合计 2,090,000 B vs 2,097,152 B，**偏差 0.34%**；单项最大 0.54% |
| 2 | 不传 `dry_run` 时零删除 | ✅ 通过 | 缺省反序列化 `dryRun=true`；清理前后目录树 SHA-256 一致，deleted=0 |
| 3 | 清理只影响 A 类；B/C 文件数 + SHA-256 一致 | ✅ 通过 | Claude B/C 11 个文件：11=11、11 个 SHA-256 一致；Codex `sessions`、Ollama `models` 零变化 |
| 4 | 伪造 cacheId / `../` / 根目录 → 全部拒绝 | ✅ 通过 | 11 类非法目标全拒（含 B 类目录冒充 cache、`../`、`/etc`、工具根、主目录、凭据）；磁盘零变化 |
| 5 | 符号链接：不跟随、不被删 | ✅ 通过 | 目录型 symlink → `Skipped/PATH_IS_SYMLINK`；文件型 symlink（`logs_*.sqlite`）→ `Skipped`；目标均保留 |
| 6 | 幂等：连续两次清理 | ✅ 通过 | 第二次 freed=0、deleted=0、skipped=1（`PATH_NOT_FOUND`）、failed=0 |
| 7 | 单项失败隔离 | ✅ 通过 | 只读目录项 `DELETE_FAILED: Permission denied`，其它项成功；失败项无半删；失败进 warnings |
| 8 | 空态：env 指向空目录 | ✅ 通过 | 全空环境 9 工具 detected=[]、0 B；空目录 env detected=3、可清理 0；缺失目标 skipped=1 |

---

## 1. 验收方法

- 后端：`src-tauri/tests/qa_ai_cache.rs`（**19 个集成用例**），通过注入式 env lookup 把 `HOME`/`XDG`/`APPDATA` 指向临时夹具，直接调用交付实现（`scan::scan_all` / `scan::scan_all_with_heavy` / `clean::clean_all` / `clean::validate_target`），无 mock 业务逻辑。
- 前端：`npm run lint` + `npm run build`（静态导出）+ 关键交互代码核对（分批、惰性计算、降级禁用、停止显示文案、受保护项）。
- 运行命令：
  ```bash
  cargo test --manifest-path src-tauri/Cargo.toml --test qa_ai_cache -- --nocapture --test-threads=1
  ```

### 1.1 本机工具链绕行（非产品缺陷）

`cargo` 直连 crates.io 长时间卡在 `Updating crates.io index`，且并发 `cargo` 会因 `.package-cache` 文件锁互等。本次用**隔离 `CARGO_HOME` + rsproxy 稀疏镜像 + 隔离 `CARGO_TARGET_DIR`** 完成验证（不修改仓库与 `.ops` 全局配置）：

```toml
[source.crates-io]
replace-with = "rsproxy-sparse"
[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"
```

建议 Ops 把该镜像写进 `.ops/env.sh` 的 `CARGO_HOME/config.toml`，避免全队再踩。

---

## 2. AC 逐条对照（spec v1.1）

> 口径说明：`通过` = 后端逻辑 + 可自动化部分实测通过；`无法完整验证` = 依赖真机 UI / 真实规模；`失败` = 规格要求的能力缺失或行为不符。

| ID | 验收项 | 结论 | 证据 / 说明 |
|----|--------|------|------------|
| AC-01 | 工具入口 | ⚠️ 无法完整验证（静态通过） | `src/lib/tools.ts:69-73` 注册且无 `windowsOnly`；`npm run build` 输出含 `/tools/ai-cache-clean`；导航数据驱动。未真机点击 |
| AC-02 | 自动检测（9 工具） | ✅ 通过 | 夹具仅建 claude/codex 根：detected=`["claude-code","codex"]`，其余 7 个 false；后端未排序、前端按可清理体积降序（`page.tsx:84-89`，见 D6 注） |
| AC-03 | 环境变量根 | ✅ 通过 | `CLAUDE_CONFIG_DIR=<tmp>/custom-claude`：扫描根切换，清理只删其中 `debug/`，默认 `~/.claude` 未被创建 |
| AC-04 | 体积准确性 vs `du -sk` ≤5% | ✅ 通过 | 合计偏差 **0.34%**；单项 plans 0.12%、debug 0.54%；白名单外/ B 类 5 MB 不计入 |
| AC-05 | 扫描性能 ≤5 s | ⚠️ 无法完整验证 | 夹具 800 文件 / 100 MB / 9 工具：≈0–1 ms（仅元数据）；真实 5 GB 冷缓存未构造。**B 类已改为惰性**，主扫描不再遍历 GB 级 sessions/models（v1 的风险已消除） |
| AC-06 | 分级展示 + B 类按需计算 | ✅ 通过 | 主扫描 sessions `measured=false`、bytes=0、`cleanable=false`、`totalDisplayOnlyBytes=0`；heavy 扫描（`includeHeavy=true`）→ measured=true、bytes=61,000、displayOnly=61,000；Sessions/ModelCache 清理被命令级拒绝。UI「计算占用」按钮 + 合并回卡片（`page.tsx:539-575`，代码核对） |
| AC-07 | 预览 + 确认，确认前零变化 | ✅ 通过 | dry-run 默认 true、目录树 SHA-256 前后一致；前端先 dry-run 预览再弹窗（绝对路径 + 合计 + 「清理后无法恢复」+ 默认焦点「取消」） |
| AC-08 | 释放量 = 清理前字节数 | ✅ 通过 | 清理前 500,000 B = freed 500,000 B，误差 0.00% |
| AC-09 | 清理后重扫归零 | ✅ 通过 | 重扫 cleanable=0 B；目录/文件两类目标均验证（B1 用例删除 `logs_1.sqlite` 文件）；UI 800ms 自动重扫（代码核对） |
| AC-10 | 白名单拒绝 | ✅ 通过 | 11 类非法目标全拒（含 B 类路径冒充 Cache、Sessions 类别、`../`、`/etc`、根、主目录、凭据、空、相对路径）；未知工具/空请求/65 项命令级拒绝；磁盘零变化 |
| AC-11 | 路径越界拒绝 | ✅ 通过 | `..` → `PATH_NOT_ALLOWED: 路径不允许包含 ..`；根外目录拒绝；链接目标不跟随（见 AC-12） |
| AC-12 | 符号链接（目录 + 文件） | ✅ 通过 | 目录型：`Skipped/PATH_IS_SYMLINK`，链接与目标保留、内部链接目标 900 KB 不计数；文件型（`logs_link.sqlite`）：`Skipped`，目标保留 |
| AC-13 | B/C 类零影响 | ✅ 部分覆盖（机制通过） | Claude：11 个 B/C 文件（projects/file-history/tasks/paste-cache/usage-data/凭据/配置/plugins/agent-memory/白名单外）文件数 + SHA-256 完全一致；Codex `sessions`、Ollama `models` 零变化且不可清理。Cursor/Windsurf `workspaceStorage` 未单独造夹具（同 Sessions 机制） |
| AC-14 | 运行中保护 | ✅ 通过 | 新写文件 `in_use=true`；`touch -t 202001010000` 的 `in_use=false`；前端默认勾选排除 `inUse`，手动可勾 |
| AC-15 | 单项失败隔离 | ✅ 通过 | 只读目录 `0555`：`Failed / DELETE_FAILED: Permission denied (os error 13)`；另一项 `Deleted`；失败项未半删；B 类不受影响；失败进 warnings |
| AC-16 | 根目录保护 | ✅ 通过 | 工具根、用户主目录均被拒且零变化。注：错误码为 `PATH_NOT_ALLOWED`（白名单先拒），`ROOT_PROTECTED` 分支仍不可达（D7） |
| AC-17 | 空态/异常态 | ⚠️ 无法完整验证 | 后端全空/空目录 env 不崩溃、0 B、缺失目标 skipped；不可读内容有 warning（「部分内容无法读取（N 项）」）并在卡片展示。UI 空态/「无权限读取」文案未真机验证（D9） |
| AC-18 | 无网络 / 无提权 | ✅ 通过（静态） | `ai_cache` 无网络/`Command`/shell 调用（grep 零命中）；无提权路径；删除统一 `std::fs` |
| AC-19 | 幂等 | ✅ 通过 | 第二次 freed=0、deleted=0、skipped=1、failed=0；期间新增 B 类文件未受影响 |
| AC-20 | 隐私 | ✅ 通过（静态） | `ai_cache` 与页面无 `localStorage`/文件写入/上传/遥测（grep 零命中） |
| AC-21 | override 降级 | ✅ 通过（错误码文案待对齐） | `$HOME` 本身 / `$HOME` 上级 / `/` 三种 override：catalog `cleanable=false` + warning、扫描结果 `cleanable=false`、清理命令级 `TOOL_DISABLED`、文件未删；普通 override 对照 `cleanable=true` 且可正常清理。⚠️ spec 写 `TOOL_DOWNGRADED`，实现为 `TOOL_DISABLED`（D-N1）；`/` 的降级原因文案误写为「主目录的上级」（D-N2） |
| AC-22 | 分批提交 | ✅ 通过（后端 + 前端代码） | 后端：64 项整批接受、65 项 `单次清理最多 64 项` 拒绝；前端 `CLEAN_BATCH_SIZE=64` + `runCleanBatches` 串行、预览（dryRun=true）与真删共用、合并 freed/deleted/skipped/failed/items/warnings（`page.tsx:111-169,577,604`）。UI 65 项实操未真机验证 |
| AC-23 | 系统缓存根精确条目（D4） | ❌ **失败（未实现）** | `catalog.rs` 中 Claude Code 只有 `~/.claude` 一个根；`grep -c 'claude-cli-nodejs' src-tauri/src/ai_cache/catalog.rs` = **0**；`~/Library/Logs/Claude`、`~/Library/Caches/Cursor`/`Windsurf` 系统缓存根同样未进入 catalog。按 spec v1.1 要求（`<项目目录>/--mcp-logs-*`、只删精确条目）**功能缺失** |

### 2.1 功能清单（spec §3.1）核对

| ID | 功能 | 结论 |
|----|------|------|
| F1 工具入口 / F2 自动检测 / F3 占用统计 / F4 分级展示 | ✅ |
| F5 B 类体积按需计算 | ✅ 本轮已实现（主扫描惰性 + heavy 二次扫描） |
| F6 确认后清理（dry-run 预览 + 路径确认） | ✅ |
| F7 结果反馈（成功/失败明细 + 自动重扫） | ✅ |
| F8 安全校验 | ✅（白名单精确匹配 + 规范化 + 链接/根/越界拒绝） |
| F9 异常隔离 / F10 运行中保护 | ✅ |
| F11 空态/错误态 | ⚠️ 后端 ✅，UI 未真机验证 |
| F12 四类 cargo test | ✅ 既有 47 + 新增集成 19 全绿 |

---

## 3. 回归记录（相对 v1 报告 139c818）

| v1 问题 | 严重程度 | 本轮状态 | 证据 |
|---|---|---|---|
| D2：AC-06 / F5 B 类体积未按需计算（主扫描递归统计 GB 级目录） | 中 | ✅ **已修复** | 主扫描 `measured=false`/bytes=0；`scan_all_with_heavy` 才统计；UI「计算占用」合并结果；测试 `ac06_*`、`ac13_multi_tool_*` |
| **B1（后续发现的阻断）**：`logs_*.sqlite` glob 可能把同名**目录**当文件项并递归删除 | 高 | ✅ **已修复并回归** | `expand_targets` 只收普通文件；`validate_target` 通配项强制 `is_file()`；删除前再查。测试 `b1_glob_only_matches_files_not_directories`：同名目录被拒且内容保留，真实文件可删，文件型 symlink 被跳过 |
| AC-21 / R9：env override 指向 `$HOME`/上级/`/` 时无降级护栏 | 中 | ✅ **已修复** | catalog/scan `cleanable=false` + warning；clean 命令级 `TOOL_DISABLED`；前端禁用勾选 + 横幅；测试 `ac21_*` |
| AC-22 / R10：>64 项会被整批拒绝（前端未分批） | 中 | ✅ **已修复** | 前端 64 分批串行（预览+真删）并合并结果；后端仍 64 上限；测试 `ac22_*` |
| S5：进度分子只算 deleted，跳过/失败时进度不到 100%；删除阶段失败不进 warnings | 中 | ✅ **已修复** | 事件 `processed_count = deleted+skipped+failed`；测试 `clean_progress_counts_cover_all_items`；失败项进 warnings（`ac15_*` 断言） |
| D3：前端回传绝对路径违反 R1「只传 toolId + cacheId」 | 中 | ✅ 已由 spec v1.1 R1 裁决（允许 `{toolId, category, path}` + 后端逐项复验），不再是偏差 | spec v1.1 §4.1 R1 |
| D5：工具矩阵超出 spec（Claude Desktop / VS Code Copilot / Ollama） | 低 | ✅ 已由 spec v1.1 纳入并裁决边界，关闭 | spec v1.1 §2.1 / §10 R6 |
| D8：plan 的 sessions 可清理 / 回收站默认与实现不一致 | 信息 | ✅ plan v1.1 已回写为「仅展示 + 永久删除」 | plan v1.1 §1/§3.1（工作区） |

**v1 结论中"后端安全硬规则全部通过"在本轮仍然成立**，且新增的 B1 修复把"文件类白名单"的攻击面补齐（同名目录不再可能被递归删除）。

---

## 4. 未关闭问题与待裁决

### 4.1【高 · 待裁决】AC-23 系统缓存根未实现 vs spec v1.1 要求纳入

- spec v1.1（工作区未提交）§4.2.1 明确：macOS `~/Library/Caches/claude-cli-nodejs` 下 `<项目目录>/--mcp-logs-*` 纳入 `Cache` 可清理（并称"472 GB 级异常增长的主要来源"），`~/Library/Logs/Claude` 整目录可清理；Cursor/Windsurf 系统缓存根同样纳入；AC-23 要求夹具验证只删精确条目、根与其它子目录 SHA-256 不变。
- 实现（`b8d6bf3`）：`catalog.rs` 只有 `~/.claude` / `~/.codex` / Application Support 等原有根，**无 `claude-cli-nodejs`，无 Caches/Cursor、Caches/Windsurf 根**。
- 影响：若以 v1.1 为验收基线 → AC-23 **不通过**；若 v1.1 只是提案 → 需把 AC-23 撤回或标 v2。
- 建议：Chief 明确基线。若要实现，`catalog` 需支持「二级动态父目录 + 精确 glob 子项」（当前 `expand_targets` 只支持根级单层 glob），并补 AC-23 夹具测试。

### 4.2【高 · 待裁决】D1：`paste-cache` / `usage-data` 的 spec 与实现方向相反

- 实现（`6d0af9f` 起）：已把它们从 `Cache` 移入只读 `Sessions`（**只展示、不可清理**），回归确认清理被拒且文件保留。
- spec v1.1（工作区）§4.2.1 A 类表**仍把 `paste-cache`、`usage-data` 列为可清理**（"只丢历史粘贴内容/insights 报告"）。
- plan v1.1 声称"已按实现回写、QA 以本版为准"，但上述两处与 HEAD 实现不符。
- 结论：**实现方向是更安全的那个，但规格文本与实现冲突，不能同时作为验收基线**。建议保留实现（不清理）并把 spec v1.1 §4.2.1/AC 里这两项移到 B 类；若产品坚持清理，则需回退实现并补风险文案 + 测试。

### 4.3【低】错误码/文案不一致

- N1：AC-21 要求 `TOOL_DOWNGRADED`，实现返回 `TOOL_DISABLED`（前端只展示中文文案，不影响功能；契约/测试断言需统一）。
- N2：override 为文件系统根 `/` 时，warning 写「根目录是用户主目录的上级」，应为「文件系统根」。
- N3（v1 遗留）：`ROOT_PROTECTED` 分支不可达（根/主目录先被白名单拒绝，返回 `PATH_NOT_ALLOWED`）；保护效果成立，仅错误码语义不精确。
- N4（v1 遗留）：无权限目录 warning 为「部分内容无法读取（N 项）」而非 spec 的「无权限读取」；排序在后端未实现（前端排序满足 AC-02）。

---

## 5. 无法验证项及原因

| 项 | 原因 |
|---|---|
| AC-01 / AC-06 / AC-07 / AC-17 / AC-21 / AC-22 的**真机 UI 表现**（导航、计算占用、确认弹窗焦点、降级禁用、分批 65 项实操） | 本机无法运行 `npm run tauri:dev`（需编译 Tauri 应用并开窗），只做静态构建 + 代码核对；建议有 GUI 的机器按 1280×800 / 900×640 补 7 状态截图 |
| AC-05 真实 5 GB / 冷缓存 | 无法安全构造 5 GB 真实缓存；100 MB 夹具验证了算法与量级（元数据遍历 1 ms）；B 类已惰性，风险显著下降 |
| AC-13 Cursor / Windsurf `workspaceStorage` 零影响 | 未单独造夹具（与 Claude `projects`、Codex `sessions`、Ollama `models` 同一 `Sessions`/`ModelCache` 机制，已覆盖机制层） |
| AC-18 断网真机 | 仅静态确认无网络/提权代码路径 |
| Windows / Linux（大小写、junction、`%APPDATA%`） | 本机仅 macOS |

---

## 6. 质量门（本轮真实输出）

```
$ cargo fmt --manifest-path src-tauri/Cargo.toml --check
[exit 0]

$ cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s)
[exit 0]

$ cargo test --manifest-path src-tauri/Cargo.toml
test result: ok. 47 passed; 0 failed   (lib)
test result: ok. 19 passed; 0 failed   (qa_ai_cache)
[exit 0]

$ npm run lint      # exit 0，无告警
$ npm run build     # exit 0，静态导出含 /tools/ai-cache-clean
```

---

## 附录 A：集成测试清单（`src-tauri/tests/qa_ai_cache.rs`，19 个）

| 测试 | 覆盖 |
|---|---|
| `ac04_scan_bytes_match_du_within_5_percent` | AC-04（逐项 + 合计 du 偏差） |
| `ac05_scan_is_fast_on_fixture` | AC-05（800 文件/100 MB <5 s） |
| `ac06_b_class_is_display_only_and_measured_lazily` | AC-06（惰性 × cleanable=false × 拒绝清理） |
| `ac07_dry_run_defaults_true_and_deletes_nothing` | AC-07（缺省 dryRun + 目录树 SHA 不变） |
| `ac08_ac09_freed_bytes_and_rescan` | AC-08 / AC-09 |
| `ac10_11_16_reject_forged_and_out_of_scope_targets` | AC-10 / 11 / 16（11 类非法目标） |
| `ac12_symlink_target_not_followed_or_deleted` | AC-12（目录型 symlink） |
| `ac13_bc_untouched_by_clean` | AC-13（Claude B/C 11 文件 SHA） |
| `ac13_multi_tool_b_class_zero_impact` | AC-13（Codex sessions / Ollama models 惰性 + 零变化） |
| `ac14_in_use_flag_from_mtime` | AC-14 |
| `ac15_partial_failure_is_isolated` | AC-15 + S5 warnings |
| `ac17_empty_state_does_not_crash` | AC-17 |
| `ac19_clean_is_idempotent` | AC-19 |
| `ac21_override_to_sensitive_root_degrades_tool` | AC-21（home / 上级 / 根 + 对照） |
| `ac22_backend_64_target_limit` | AC-22（64 接受 / 65 拒绝） |
| `b1_glob_only_matches_files_not_directories` | R3 / B1 + AC-12 文件型 symlink |
| `clean_progress_counts_cover_all_items` | S5 进度 + 跳过 |
| `ac02_ac03_detection_and_env_override` | AC-02 / AC-03 |
| `catalog_command_shape_and_real_env` | catalog 契约 + 真实 env |

## 附录 B：关键原始输出（本轮，节选）

```
AC-04 合计 scan=2090000B du=2097152B 偏差=0.34%（阈值 5%）
AC-06 主扫描 sessions measured=false bytes=0 cleanable=false
AC-06 heavy 扫描 sessions measured=true bytes=61000 displayOnly=61000
AC-06 Sessions 清理拒绝：CATEGORY_NOT_CLEANABLE: 会话记录 类别在 v1 仅展示，不提供清理
AC-07 缺省 dryRun 反序列化结果 dryRun=true
AC-10 拒绝 B 类目录当 cache 类别清理 -> status=Failed error=PATH_NOT_ALLOWED: 路径不在白名单目录内
AC-10 超限错误：INVALID_ARGUMENT: 单次清理最多 64 项
AC-12 清理结果 deleted=1 skipped=1 （image-cache=Deleted, plans 内链接目标不计数 900000B）
AC-13 B/C 类文件数 before=11 after=11 含 SHA-256 的文件数=11
AC-13 heavy displayOnly=605000（Codex sessions 5000 + Ollama models 600000）
AC-13 Ollama models 清理拒绝：CATEGORY_NOT_CLEANABLE: 模型文件 类别在 v1 仅展示，不提供清理
AC-15 失败原因：DELETE_FAILED: 删除失败：Permission denied (os error 13)
AC-19 第二次 freed=0 deleted=0 skipped=1 failed=0
AC-21 $HOME 本身 清理拒绝：TOOL_DISABLED: 工具 Claude Code 根目录指向用户主目录…
AC-22 65 项拒绝：INVALID_ARGUMENT: 单次清理最多 64 项
B1 扫描 logs 项：["logs_1.sqlite"]（同名目录被排除）
B1 清理目录结果 deleted=0 failed=1 error=PATH_NOT_ALLOWED: 通配白名单项只允许删除普通文件，目标不是文件
B1 清理文件结果 deleted=1 freed=100
B1 文件符号链接结果 deleted=0 skipped=1 status=Skipped
S5 最后事件 processed=2 freed=100
```

## 附录 C：与 v1 报告的结论变化

- AC 结果：v1「通过 16 / 失败 1（AC-06）/ 无法验证 3」→ 本轮「通过 19 / 失败 1（AC-23）/ 无法完整验证 3」（口径升级到 spec v1.1，新增 AC-21/22 通过、AC-23 失败）。
- 必测 8 项：7/8 → **8/8**。
- 新发现并已修复：B1（glob 通配命中目录 → 递归删除风险），已由 Coder 修复并加入本报告回归测试。
- 仍未解决：AC-23 未实现、spec v1.1 与实现对 `paste-cache`/`usage-data` 的方向冲突、3 处低危文案/错误码（N1–N4）、真机 UI 未验。

---

**签署**：Tester · 2026-10-31
**建议**：先由 Chief 收敛「spec v1.1 是否作为验收基线（AC-23 / paste-cache 两项）」；Coder 修 AC-23（或撤回）与 N1/N2 后，我按附录 A 全量回归并出最终通过结论。
