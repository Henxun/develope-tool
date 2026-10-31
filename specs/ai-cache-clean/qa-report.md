# QA 最终验收报告 — AI 智能体缓存清理（v1）

- 执行人：Tester（测试）
- 时间：2026-10-31（Asia/Shanghai）
- 验收基线：**spec v1.4（已提交 `154a71d`，文档与实现对齐：D1 回写、错误码 `TOOL_DISABLED`；D4 在 v1.1 backlog；AC-01~AC-22）**
- 代码基线：`feat/ai-cache-clean` @ **`8df1201`（最终 HEAD）**——含 `89dac87`（D1）、`b8d6bf3`（B1/惰性计算/降级/分批）、`e991341`（heavy 扫描并发口 + 文件系统根文案）、`5a6fa30`（计算占用串行约束 + 分批失败兜底）、`8df1201`（9 工具矩阵字面断言）；另有 spec v1.4 等 docs 提交
- 验证环境：macOS（Darwin，Apple Silicon）、rustc/cargo 1.99.1、Node v24.21.0
- 验收方式：Rust 集成测试（真实实现 + 临时夹具 + 注入 env），前端 lint/build + 关键交互代码核对

---

## 0. 最终结论

### 0.1 计分（spec v1.3 共 22 条 AC）

| 结论 | 条数 | AC |
|---|---|---|
| ✅ **通过** | **19** | AC-02、03、04、06、07、08、09、10、11、12、13、14、15、16、18、19、20、21、22 |
| ❌ **失败** | **0** | — |
| ⚠️ **无法验证（本机条件不具备）** | **3** | AC-01、AC-05、AC-17 |
| 备注：backlog（不计入 v1 验收） | 1 | B-AC-23（D4 系统缓存根，spec v1.3 已移入 §12 v1.1 backlog，代码未实现属预期） |

**必测 8 项（Chief 指定）：8/8 通过。** 后端安全硬规则、D1 只展示、D5 受保护项、B/C 类零影响、dry-run 默认保护、幂等、失败隔离、空态，均有自动化实测证据。

### 0.2 本轮裁决项的验收结论

| 裁决 | 结论 | 证据 |
|---|---|---|
| **D1**：`paste-cache` / `usage-data` 只展示不清理 | ✅ 通过 | 两者归入 `Sessions`（`cleanable=false`）；用 `Cache` 类别伪造提交 → `PATH_NOT_ALLOWED`；用 `Sessions` 类别 → 命令级 `CATEGORY_NOT_CLEANABLE`；清理后文件与 SHA-256 零变化（`d1_*`、`ac06_*`、`ac13_*`） |
| **D4**：系统缓存根不进 v1 | ✅ 符合基线（不计验收） | `catalog.rs` 中无 `claude-cli-nodejs` 等系统缓存根（grep 0 命中），与 v1.3「不进 v1」一致；原 AC-23 已移 backlog |
| **D5**：9 工具矩阵保留；Claude Desktop 敏感项受保护/仅展示 | ✅ 通过（1 处展示清单小缺口） | 9 工具均在 catalog；Claude Desktop 的 `claude_desktop_config.json` / `Preferences` / `Cookies` / `Network` / `extensions` / `config.json` / `state*` **逐项实测清理被拒且文件零变化**，扫描中不作为可清理项返回；仅 `protected` 展示清单缺少 `extensions`/`config.json`/`state*`（见 N5，无删除风险） |

### 0.2.1 D1 页面回归：「仅展示」区仍可见且可「计算占用」（不静默消失）

- 后端数据：scan 在未计算时也把两目录列在 `Sessions.paths`（供展开 / 「打开目录」），`measured=false`；heavy 扫描后两目录 `bytes>0`（D1 用例断言合计 4,000 B）。
- 前端（静态核对 + build）：`tool-cache-card.tsx:45` 按 `cleanable` 拆分 `readonlyCategories`，`:189-205` 渲染「会话与模型数据（仅展示，不清理）」区块；`category-section.tsx:97-119` 对不可清理类别显示「仅展示」徽标与「计算占用」按钮；`page.tsx:540-575` 计算占用时调 `scanAiCacheUsage([toolId], true)` 并把结果合并回卡片（`:810` 绑定）；`e991341` 起 heavy 计算期间禁用重新扫描，避免两路进度混入同一条进度条。
- 未验：真实 Tauri 页面点按（需开窗应用），按 §4 记为「无法验证」。

### 0.3 质量门（最终 HEAD `8df1201` 实测）

```
cargo fmt --check                                   → exit 0
cargo clippy --all-targets -- -D warnings           → exit 0，0 warning
cargo test                                          → lib 48 passed / qa_ai_cache 22 passed / 0 failed
npm run lint                                        → exit 0
npm run build                                       → exit 0（静态导出含 /tools/ai-cache-clean）
```

---

## 1. AC 逐条对照（spec v1.3）

> 判定口径：`通过` = 后端逻辑与可自动化部分实测通过；`无法验证` = 本机条件不具备（已写原因）；`失败` = 规格要求的能力缺失或行为不符。

| ID | 验收项 | 结论 | 证据 / 说明 |
|----|--------|------|------------|
| AC-01 | 工具入口 | ⚠️ **无法验证** | 原因：本机无法运行 `npm run tauri:dev`（需编译并开窗 Tauri 应用），无法做真机导航/页面走查。可做的静态证据：`src/lib/tools.ts` 已注册且无 `windowsOnly`；`npm run build` 静态导出包含 `/tools/ai-cache-clean`；导航由 `useVisibleTools()` 数据驱动 |
| AC-02 | 自动检测（9 工具） | ✅ 通过 | 只建 claude/codex 根 → detected 恰好 `["claude-code","codex"]`，其余 7 个 false；前端按可清理体积降序（`page.tsx:84-89`，后端不排序，见 N4） |
| AC-03 | 环境变量根 | ✅ 通过 | `CLAUDE_CONFIG_DIR=<tmp>/custom-claude`：检测根切换，清理只删其中 `debug/`，默认 `~/.claude` 未被创建 |
| AC-04 | 体积准确性 vs `du -sk` ≤5% | ✅ 通过 | 合计 2,090,000 B vs 2,097,152 B，偏差 **0.34%**；单项最大 0.54%；B 类 5 MB 不计入 |
| AC-05 | 扫描性能 ≤5 s | ⚠️ **无法验证** | 原因：本机无真实 AI 工具目录，无法构造 "A 类合计 <5 GB + 冷缓存" 场景；仅 800 文件/100 MB 夹具实测 ≈1 ms（只做元数据遍历）。B 类已惰性（主扫描不遍历 GB 级 sessions/models），风险已下降，但真实性能未验 |
| AC-06 | 分级展示 + B 类按需计算 | ✅ 通过 | 主扫描 sessions `measured=false`、bytes=0、`cleanable=false`、`totalDisplayOnlyBytes=0`；heavy 扫描 → measured=true、61,000 B；Sessions/ModelCache 清理命令级拒绝；UI「计算占用」按钮 + 合并回卡片（代码核对）；**AC-06b 10 万 B 类文件证据**：`projects/` 下 100×1,000 个 8 B 文件（合计 800,000 B），主扫描 duration=0ms / displayOnly=0 / cleanable=1,000 B，heavy 281ms / displayOnly=800,000 B——主扫描确实不遍历 B 类 |
| AC-07 | 预览 + 确认 | ✅ 通过 | 缺省 `dryRun=true`；dry-run 前后目录树 SHA-256 完全一致；前端先 dry-run 预览再弹窗（逐条绝对路径 + 合计 + 「清理后无法恢复」+ 默认焦点「取消」） |
| AC-08 | 释放量 | ✅ 通过 | 清理前 500,000 B = freed 500,000 B，误差 0.00% |
| AC-09 | 清理后重扫 | ✅ 通过 | 重扫 cleanable=0 B；目录类与文件类目标均验证（`logs_1.sqlite` 删除后文件不存在）；UI 800ms 自动重扫（代码核对） |
| AC-10 | 白名单拒绝 | ✅ 通过 | 11 类非法目标全拒：B 类目录冒充 cache（`PATH_NOT_ALLOWED`）、Sessions 类别（`CATEGORY_NOT_CLEANABLE`）、`../`、`/etc`、工具根、主目录、白名单外目录、凭据文件、空路径、相对路径；未知工具/空请求/65 项命令级拒绝；磁盘零变化 |
| AC-11 | 路径越界拒绝 | ✅ 通过 | `..` → `PATH_NOT_ALLOWED: 路径不允许包含 ..`；根外路径、被替换链接均拒绝 |
| AC-12 | 符号链接（目录 + 文件） | ✅ 通过 | 目录型 symlink → `Skipped/PATH_IS_SYMLINK`（链接与目标都保留，内部链接目标 900 KB 不计数）；文件型 symlink（`logs_link.sqlite`）→ `Skipped`，目标保留 |
| AC-13 | B/C 类零影响 | ✅ 通过 | Claude：11 个 B/C 文件文件数 11=11、SHA-256 11 个全部一致；Codex `sessions`、Ollama `models`、Cursor `User/workspaceStorage` 零变化且不可清理（伪造 cache 类别也被拒）；Cursor A 类 `Cache` 可正常清理 |
| AC-14 | 运行中保护 | ✅ 通过 | 新写文件 `in_use=true`；`touch -t 202001010000` 的 `in_use=false`；前端默认勾选排除 `inUse`，手动可勾选后仍可清理 |
| AC-15 | 单项失败隔离 | ✅ 通过（macOS 路径） | 只读目录 `0555`：`Failed / DELETE_FAILED: Permission denied`；另一项 `Deleted`；失败项未半删；B 类不受影响；失败进 warnings。Windows「独占打开文件」导致失败的场景本机无 Windows，未验证 |
| AC-16 | 根目录保护 | ✅ 通过 | 工具根、用户主目录均被拒且零变化。注：实际错误码为 `PATH_NOT_ALLOWED`（白名单先拒），`ROOT_PROTECTED` 分支不可达（N2，保护效果成立） |
| AC-17 | 空态/异常态 | ⚠️ **无法验证** | 原因：UI 无法真机走查（同上）。后端已实测：全空环境 9 工具 detected=[]、0 B、无 panic；空目录 env detected=3、可清理 0；缺失目标 skipped=1；不可读内容返回 warning「部分内容无法读取（N 项）」并在卡片展示（代码核对）。spec 要求文案「无权限读取」与实现文案不同（N3） |
| AC-18 | 无网络 / 无提权 | ✅ 通过（静态） | `ai_cache` 无网络/`Command`/shell 调用（grep 零命中）、无提权路径、删除统一 `std::fs`。未做断网真机（本机环境限制） |
| AC-19 | 幂等 | ✅ 通过 | 第二次清理 freed=0、deleted=0、skipped=1（`PATH_NOT_FOUND`）、failed=0，无错误；期间新增 B 类文件未受影响 |
| AC-20 | 隐私 | ✅ 通过（静态） | `ai_cache` 与页面无 `localStorage`/文件写入/上传/遥测（grep 零命中） |
| AC-21 | override 降级 | ✅ 通过 | `$HOME` 本身 / `$HOME` 上级 / `/`：catalog+scan `cleanable=false`+warning，清理命令级拒绝（`TOOL_DISABLED`，spec v1.4 已对齐），文件零变化；普通 override 对照可正常清理 |
| AC-22 | 分批提交 | ✅ 通过（后端 + 前端代码） | 后端 64 项接受、65 项拒绝；前端 `CLEAN_BATCH_SIZE=64` 串行分批（预览与真删共用 `runCleanBatches`）并合并 freed/deleted/skipped/failed/items/warnings。65 项真机 UI 操作未走查（见 AC-01 原因） |

### 1.1 v1.1 backlog（不计入 v1 验收）

| ID | 项 | 状态 |
|---|---|---|
| B-AC-23 | macOS 系统缓存根精确条目（`claude-cli-nodejs` / `<proj>/--mcp-logs-*`、`~/Library/Logs/Claude`、`~/Library/Caches/Cursor`、`~/.cache` 等） | **未实现，符合 v1.3 裁决**（移入 v1.1 backlog）；代码中无相关根（grep 0 命中），v1 不扫描不清理 |

---

## 2. 必测 8 项（Chief 指定）逐项结果

| # | 必测项 | 结论 | 证据 |
|---|---|---|---|
| 1 | 扫描体积 vs `du -sk` ≤5% | ✅ | 偏差 0.34%（合计），单项 ≤0.54% |
| 2 | 不传 `dry_run` 时零删除 | ✅ | 缺省 `dryRun=true`；SHA-256 目录树前后一致、deleted=0 |
| 3 | 只影响 A 类，B/C 零影响 | ✅ | 11 个 Claude B/C 文件 SHA-256 一致；Codex/Ollama/Cursor B 类均不可清理、零变化 |
| 4 | 伪造 cacheId / `../` / 根目录 → 拒绝 | ✅ | 11 类非法目标全拒，磁盘零变化；含 D1 两项伪造类别 |
| 5 | 符号链接不跟随、不删 | ✅ | 目录型 + 文件型 symlink 均 `Skipped`，目标保留 |
| 6 | 幂等 | ✅ | 第二次 freed=0、无错误、无新删除 |
| 7 | 单项失败隔离 | ✅ | 只读目录项失败 + 其它项成功 + 失败进 warnings，无半删 |
| 8 | 空态不崩溃 | ✅ | 全空环境 9 工具 detected=[]、0 B、无 panic |

---

## 3. 需要 Product/Coder 处理的非阻塞项（均不影响 v1 结论）

| # | 级别 | 内容 | 建议 |
|---|---|---|---|
| N1 | ✅ 已解决 | AC-21 错误码：spec v1.4 已把 `TOOL_DOWNGRADED` 改为 `TOOL_DISABLED`，与实现一致 | — |
| N2 | ✅ 已解决 | 「`/` 降级文案误写为主目录上级」已在 `e991341` 修复（最终实测 warning=「根目录指向文件系统根，已降级为仅检测」）。`ROOT_PROTECTED` 分支仍不可达，但保护效果成立 | 错误码语义如需精确可后续统一 |
| N3 | 低 | 无权限目录 warning 为「部分内容无法读取（N 项）」，spec 要求「无权限读取」；AC-17 UI 文案未真机验 | 统一文案后补 UI 走查 |
| N4 | 低 | 工具卡排序在前端实现，后端 `<scan>` 返回 catalog 顺序；AC-02 在 UI 层满足 | 文档注明或后端排序 |
| N5 | 低 | Claude Desktop `protected` 展示清单缺少 `extensions` / `config.json` / `state*`（spec §4.2.2 列为「仅展示/受保护」）；三者**不可清理、清理被拒**，仅 UI 不展示 | 补 protected 条目（1 行配置） |
| N6 | ✅ 已解决 | spec v1.4 已把 `paste-cache` / `usage-data` 移到 B 类（`Sessions`，只展示、无清理入口），与实现一致 | — |

> 剩余 **N3（无权限文案）/N4（排序在后端未实现）/N5（Claude Desktop protected 展示清单缺 3 项）** 均为低危/文档类，**不构成 AC 失败**。最终 HEAD `5a6fa30` 另修：`runCleanBatches` 某批命令级失败时保留前序批次结果并明确提示「第 x 批失败，前 y 批已生效」；「计算占用」在后台扫描未结束时不再并发请求（前端代码核对，真机 UI 未验证）。

---

## 4. 无法验证项及原因（不为好看标通过）

| 项 | 本机无法验证的原因 |
|---|---|
| AC-01 工具入口 / 导航「最近使用工具」 | 需要运行桌面应用（`npm run tauri:dev`/安装包）做真机点击；本机只能构建静态导出，无法开 Tauri 窗口。已用「构建产物含路由 + 注册表代码」做静态验证 |
| AC-05 真实 5 GB / 冷缓存扫描性能 | 本机无真实 AI 工具安装，构造 5 GB 缓存需大量磁盘与时间且不能代表真实 IO；仅用 100 MB/800 文件夹具验证算法与量级 |
| AC-17 UI 空态 / 无权限态 | 同 AC-01，无法真机走查；后端空态/无权限行为已实测，前端展示仅代码核对 |
| AC-15/AC-16/AC-21/AC-22 的 Windows 专属行为与真机 UI | 本机为 macOS：Windows junction、独占打开文件、`%APPDATA%` 大小写行为、65 项分批的 UI 实操均无法在本机验证；Unix 路径已实测 |
| AC-18 断网真机 | 未在断网环境跑全流程；仅静态确认无网络代码路径（团队网络/提权策略下不做破坏性断网实验） |

**真机 UI 走查建议**（交给有 GUI 的机器/人）：1280×800 与 900×640 两个尺寸，覆盖 7 个状态（初始/扫描中/有结果/空态/确认弹窗/清理中/结果），重点看：B 类「计算占用」、降级横幅与禁用勾选、65 项分批进度、确认弹窗初始焦点在「取消」。

---

## 5. 回归与证据（本轮真实输出）

### 5.1 关键数据

```
AC-04 合计 scan=2090000B du=2097152B 偏差=0.34%（阈值 5%）
AC-06 主扫描 sessions measured=false bytes=0 cleanable=false
AC-06 heavy 扫描 sessions measured=true bytes=61000 displayOnly=61000
AC-07 缺省 dryRun 反序列化结果 dryRun=true；dry-run 后 SHA-256 一致
AC-08 清理前 scan=500000B freed=500000B 误差=0.00%
AC-10 拒绝 B 类目录当 cache 类别清理 -> PATH_NOT_ALLOWED: 路径不在白名单目录内
AC-10 拒绝 sessions 类别 -> CATEGORY_NOT_CLEANABLE: 会话记录 类别在 v1 仅展示，不提供清理
AC-10 超限错误：INVALID_ARGUMENT: 单次清理最多 64 项
AC-12 清理结果 deleted=1 skipped=1（symlink 项 PATH_IS_SYMLINK，链接目标 900000B 未计数）
AC-13 主扫描 codex.sessions/ollama.models/cursor.sessions 均 measured=false cleanable=false
AC-13 heavy displayOnly=612000（5000+600000+7000）
AC-13 B/C 类文件数 before=11 after=11 含 SHA-256 的文件数=11
AC-15 失败原因：DELETE_FAILED: 删除失败：Permission denied (os error 13)
AC-19 第二次 freed=0 deleted=0 skipped=1 failed=0
AC-21 $HOME 本身 清理拒绝：TOOL_DISABLED: 工具 Claude Code 根目录指向用户主目录…
AC-21 文件系统根 /：cleanable=false，warning=「根目录指向文件系统根，已降级为仅检测」（e991341 修复 N2），清理命令级拒绝
AC-22 64 项 dry-run 接受；65 项拒绝（单次清理最多 64 项）
B1 扫描 logs 项：["logs_1.sqlite"]；同名目录被排除
B1 清理同名目录 deleted=0 failed=1（PATH_NOT_ALLOWED: 通配白名单项只允许删除普通文件）
B1 清理真实文件 deleted=1；文件型 symlink deleted=0 skipped=1
D1 paste-cache/usage-data：Sessions 只读，清理被拒、文件零变化
D5 Claude Desktop：7 个敏感路径（config/Preferences/Cookies/Network/extensions/config.json/state）全部清理被拒、零变化；Cache 可清 1000B
S5 最后事件 processed=2 freed=100（processed = deleted+skipped+failed）
```

### 5.2 测试清单（`src-tauri/tests/qa_ai_cache.rs`，22 个）

| 测试 | 覆盖 |
|---|---|
| `ac04_scan_bytes_match_du_within_5_percent` | AC-04 |
| `ac05_scan_is_fast_on_fixture` | AC-05（夹具量级） |
| `ac06_b_class_is_display_only_and_measured_lazily` | AC-06 |
| `ac06b_main_scan_does_not_traverse_b_class` | AC-06 惰性（10 万 B 类文件：主扫描 0ms / displayOnly=0；heavy 281ms / 800,000 B） |
| `ac07_dry_run_defaults_true_and_deletes_nothing` | AC-07 |
| `ac08_ac09_freed_bytes_and_rescan` | AC-08/09 |
| `ac10_11_16_reject_forged_and_out_of_scope_targets` | AC-10/11/16 |
| `ac12_symlink_target_not_followed_or_deleted` | AC-12（目录型） |
| `ac13_bc_untouched_by_clean` | AC-13（Claude B/C SHA） |
| `ac13_multi_tool_b_class_zero_impact` | AC-13（Codex/Ollama/Cursor） |
| `ac14_in_use_flag_from_mtime` | AC-14 |
| `ac15_partial_failure_is_isolated` | AC-15 + S5 warnings |
| `ac17_empty_state_does_not_crash` | AC-17（后端） |
| `ac19_clean_is_idempotent` | AC-19 |
| `ac21_override_to_sensitive_root_degrades_tool` | AC-21 |
| `ac22_backend_64_target_limit` | AC-22（后端） |
| `b1_glob_only_matches_files_not_directories` | R3/B1 + AC-12（文件型） |
| `clean_progress_counts_cover_all_items` | S5 进度 |
| `d1_paste_cache_and_usage_data_are_display_only` | D1 |
| `d5_claude_desktop_sensitive_items_are_not_cleanable` | D5 |
| `ac02_ac03_detection_and_env_override` | AC-02/03 |
| `catalog_command_shape_and_real_env` | catalog 契约 + 真实 env |

### 5.3 本机工具链说明（非产品缺陷）

`cargo` 直连 crates.io 会卡在索引更新、并发 cargo 因 `.package-cache` 锁互等。本次用隔离 `CARGO_HOME` + rsproxy 稀疏镜像 + 隔离 `CARGO_TARGET_DIR` 完成验证（不改仓库与 `.ops` 全局配置）。建议 Ops 把镜像写入 `.ops/env.sh` 的 `CARGO_HOME/config.toml`。

---

## 6. 签署

**结论（最终 HEAD `8df1201`，spec v1.4）：通过 19 条 / 失败 0 条 / 无法验证 3 条（本机无 GUI 走查与真实 5 GB 场景）；必测 8/8 通过；质量门全绿。** D1、D5 已按裁决验证通过，D4 已按裁决排除（v1.1 backlog）。剩余 N3/N4/N5 为低危文案/展示项，不阻塞交付；真机 UI 走查由有 GUI 的机器补 7 状态截图后即可交付。

Tester · 2026-10-31
