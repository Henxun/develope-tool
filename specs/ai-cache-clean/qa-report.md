# QA 验收报告 — AI 智能体缓存清理（v1）

- 执行人：Tester（测试）
- 时间：2026-10-31（Asia/Shanghai）
- 被验分支：`feat/ai-cache-clean`
  - 交付提交：`dd34992 feat(ai-cache-clean): 新增 AI 智能体缓存清理工具页`、`c146111 feat(ai-cache-clean): 新增 Rust 侧扫描/清理模块与白名单命令`
  - 测试配合提交：`27ae4dd chore(ai-cache-clean): 公开 ai_cache 模块供集成测试调用`（仅可见性，无行为变化）
- 验收依据：`specs/ai-cache-clean/spec.md` §7（AC-01…AC-20）、§3.1 功能清单、§4 白名单与硬规则；`plan.md` 作为实现口径参考
- 验收环境：macOS（Darwin，Apple Silicon）、rustc/cargo 1.99.1、Node v24.21.0
- 环境限制（重要）：
  1. 本机**没有真实 AI 工具目录**，全部用临时夹具 + 环境变量覆盖（`CLAUDE_CONFIG_DIR` / `CODEX_HOME` / `GEMINI_CLI_HOME`）验证；
  2. 无法运行 `npm run tauri:dev`（需编译并开窗 Tauri 应用），**所有 UI 表现只做静态构建 + 代码核对**，未做真机点击；
  3. 因此本报告的"通过"仅代表**后端逻辑 + 可自动化部分**成立。

---

## 0. 结论速览

| 维度 | 结果 |
|---|---|
| AC-01…AC-20（20 条） | **通过 16 条**、**失败 1 条**（AC-06）、**无法完整验证 3 条**（AC-01 / AC-05 / AC-17，均因 UI 或真实规模不可用） |
| 必测 8 项（Chief 指定） | **7 项通过，1 项不通过**（第 3 项"B 类体积按需计算"= AC-06 未实现） |
| 自动化测试 | 集成测试 **13/13 通过**；既有单测 **36/36 通过**；`cargo test` 全绿 |
| 质量门 | `cargo fmt --check` ✅、`cargo clippy --all-targets -D warnings` ✅、`cargo test` ✅、`npm run lint` ✅、`npm run build` ✅ |
| 需 Coder 修或产品裁决 | 高 1 项（spec 与 plan 分类冲突：`paste-cache` / `usage-data`）、中 3 项、低 3 项、信息 2 项 |

**一句话结论**：安全底线（白名单、越界、符号链接、根目录、B/C 类零影响、幂等、失败隔离、dry-run 默认保护）全部经自动化实测通过，**可以进入 PR 评审**；但 `paste-cache` / `usage-data` 的分类与 spec 冲突需要产品拍板，B 类体积"按需计算"（AC-06）未实现，另有一处 IPC 契约与 spec 不一致。

### 必测 8 项结果

| # | 必测项 | 结论 | 关键证据 |
|---|---|---|---|
| 1 | 扫描体积 vs `du -sk` 偏差 ≤5% | ✅ 通过 | 合计 scan=2,090,000 B / du=2,097,152 B，**偏差 0.34%**；单项最大 0.54% |
| 2 | 不传 `dry_run` 时零删除 | ✅ 通过 | 缺省反序列化 `dryRun=true`；目录树 SHA-256 前后完全一致，deleted=0 |
| 3 | 清理只影响 A 类；B/C 文件数 + SHA-256 一致 | ✅ 通过（按 plan 白名单口径） | 9 个 B/C 文件，清理前后文件数 9=9、9 个 SHA-256 全部一致；⚠️ 但 spec 与 plan 对 `paste-cache`/`usage-data` 分类冲突，见 D1 |
| 4 | 伪造 cacheId / `../` 越界 / 根目录 → 全部拒绝 | ✅ 通过 | 11 类非法目标全部被拒（含 `claude.projects`、`../`、`/etc`、工具根、主目录、凭据文件、空/相对路径），磁盘零变化 |
| 5 | 符号链接：不跟随、不被删 | ✅ 通过 | 目标 `paste-cache` 为 symlink → `Skipped/PATH_IS_SYMLINK`，链接与目标文件均保留；目录内 symlink 目标 900,000 B 未计入 |
| 6 | 幂等：连续两次清理 | ✅ 通过 | 第二次 freed=0、deleted=0、skipped=1（`PATH_NOT_FOUND`，R6 合理）、无错误，期间新增 B 类文件未被删 |
| 7 | 单项失败隔离 | ✅ 通过 | 只读目录项 `DELETE_FAILED: Permission denied (os error 13)`，另一项正常删除，整体仍返回成功明细 |
| 8 | 空态：env 指向空目录 | ✅ 通过 | 全空环境 tools=9、detected=[]、字节 0，无 panic；显式空目录 env 下 detected=3、可清理 0 |

---

## 1. 验收方法

### 1.1 后端逻辑（自动化，13 个集成测试）
新增 `src-tauri/tests/qa_ai_cache.rs`，通过**注入式 env lookup** 把 `HOME`/`XDG`/`APPDATA` 指到临时夹具，再调用交付的真实实现（`scan::scan_all` / `clean::clean_all` / `clean::validate_target`），无任何 mock 逻辑：

```bash
# 运行（需串行，避免真实 env 读取用例互相干扰）
cargo test --manifest-path src-tauri/Cargo.toml \
  --test qa_ai_cache -- --nocapture --test-threads=1
```

覆盖：AC-02/03/04/05/07/08/09/10/11/12/13/14/15/16/17/19 + catalog 契约。

### 1.2 前端（静态与构建级）
- `npm run lint`：exit 0，无告警；
- `npm run build`：exit 0，静态导出包含 `/tools/ai-cache-clean` 路由；
- 代码核对：路由注册（`src/lib/tools.ts` 无 `windowsOnly`）、默认勾选逻辑、确认弹窗、排序、重扫、错误文案等。

### 1.3 未做（原因见 §5）
真机 UI 点击、macOS 真机 `~/Library/Caches/claude-cli-nodejs` 扫描、5 GB 真实规模扫描、断网真机、Windows/Linux 平台。

---

## 2. AC 逐条对照（spec §7）

| ID | 验收项 | 结论 | 证据 / 说明 |
|----|--------|------|------------|
| AC-01 | 工具入口 `/tools/ai-cache-clean` | ✅ 通过（静态） | `src/lib/tools.ts:69-73` 注册且**无** `windowsOnly`；`npm run build` 输出含该路由；导航由 `useVisibleTools()` 数据驱动，三平台可见。**未做真机点击** |
| AC-02 | 自动检测集合（根存在才出现） | ✅ 通过 | 夹具仅建 claude/codex 根：detected=`["claude-code","codex"]`，其余 7 个 false。⚠️ 后端返回顺序为 catalog 顺序，**降序排序在前端** `page.tsx:84-89`（见 D6） |
| AC-03 | 环境变量根切换 | ✅ 通过 | `CLAUDE_CONFIG_DIR=<tmp>/custom-claude`：扫描根切换到该目录（`claude.root=…/custom-claude=true`），清理只删其中的 `debug/`，默认 `~/.claude` 未被创建 |
| AC-04 | 体积准确性 vs `du -sk` ≤5% | ✅ 通过 | 单项：paste-cache 0.33%、image-cache 0.12%、debug 0.54%；合计 **0.34%**（2,090,000 B vs 2,097,152 B）；白名单外 5 MB 未计入 |
| AC-05 | 扫描性能 ≤5 s（6 工具、<5 GB） | ⚠️ 部分验证 | 夹具 800 文件 / 100 MB / 9 工具：**1 ms**（仅元数据遍历）。真实 5 GB 冷缓存无法构造；另见 D2 风险（B 类被主扫描递归，真实机器可能超预算） |
| AC-06 | B 类"计算占用"按需（≤10 s） | ❌ **失败** | 无 `measure_tool_data` 命令、前端无「计算占用」按钮；B 类体积在**主扫描**中已递归统计并返回 `displayOnlyBytes`（见 D2） |
| AC-07 | 清理需确认；确认前零变化 | ✅ 通过 | dry-run 与真实清理共用同一校验；缺省 `dryRun=true`；目录树（含 SHA-256）前后一致。确认弹窗逐条列绝对路径 + 合计 + "清理后无法恢复" + 初始焦点在「取消」（代码核对 `clean-confirm-dialog.tsx`） |
| AC-08 | `freed_bytes` = 清理前字节数（≤1%） | ✅ 通过 | 清理前 scan=500,000 B，freed=500,000 B，误差 **0.00%** |
| AC-09 | 清理后重扫归零 | ✅ 通过 | 重扫 cleanable=0 B；目录已不存在；UI 清理后 800 ms 自动重扫（代码核对 `page.tsx:486-489`） |
| AC-10 | 白名单拒绝（伪造 id、`../`、空、未知） | ✅ 通过 | 11 类非法目标全部拒绝（明细见附录 B），磁盘零变化；命令级拒绝：未知工具 `TOOL_NOT_FOUND`、空请求 `INVALID_ARGUMENT`、65 项 `单次清理最多 64 项`。注：实现用 `path` 而非 `cacheId`，见 D3 |
| AC-11 | 路径越界拒绝（规范化后不在根内） | ✅ 通过 | `<root>/../outside/secret.txt` → `PATH_NOT_ALLOWED: 路径不允许包含 ..`；`/etc`、白名单外目录、替换为链接均被拒 |
| AC-12 | 符号链接不跟随、不删、标注 | ✅ 通过 | 清理：`Skipped` + `PATH_IS_SYMLINK: 已跳过符号链接，不删除链接目标`，链接与目标文件保留；扫描：warning `已跳过符号链接：…`，目录内 symlink 目标 900 KB 未计入 |
| AC-13 | B/C 类零影响（文件数 + SHA-256） | ✅ 通过*（按 plan 白名单口径） | 清理 2 个 A 类目录后，抽查 9 个 B/C 文件（projects/file-history/tasks/plugins/agent-memory/unknown-dir/.credentials.json/settings.json）：文件数 9=9，**9 个 SHA-256 完全一致**；工具根保留。⚠️ *spec §4.3 把 `paste-cache`/`usage-data` 归入"绝不触碰"，实现却可清理 → D1 |
| AC-14 | 运行中保护（10 分钟内修改） | ✅ 通过 | 新写文件 `in_use=true`；`touch -t 202001010000` 的目录 `in_use=false`；前端默认勾选排除 `inUse`（`page.tsx:102`、`:427`），手动勾选后仍可清理（后端不拦截，符合 spec） |
| AC-15 | 单项失败隔离 | ✅ 通过 | 只读目录 `0555` 中文件：该项 `Failed / DELETE_FAILED: 删除失败：Permission denied (os error 13)`；另一项 `Deleted`；失败项未被半删；B 类不受影响 |
| AC-16 | 根目录保护 | ✅ 通过 | 工具根、用户主目录均被拒且磁盘零变化。注：实际错误码为 `PATH_NOT_ALLOWED`（白名单先拒），`ROOT_PROTECTED` 分支不可达（见 D7） |
| AC-17 | 空态/异常态不崩溃 | ⚠️ 部分验证 | 后端：全空环境 9 工具 detected=[]、0 B、无 panic；不存在目标 skipped=1；不可读内容返回 warning「部分内容无法读取（N 项）」并在卡片展示（代码核对）。**UI 空态/无权限文案未做真机验证**，且文案不是 spec 的「无权限读取」（见 D9） |
| AC-18 | 无网络 / 无提权 | ✅ 通过（静态） | `ai_cache` 模块无任何网络/`Command`/shell 调用（grep 零命中）；无提权代码路径；删除统一走 `std::fs::remove_dir_all`。未做断网真机验证 |
| AC-19 | 幂等 | ✅ 通过 | 第二次 freed=0、deleted=0、skipped=1（`PATH_NOT_FOUND`）、failed=0；期间新增 B 类文件未被影响 |
| AC-20 | 隐私（不落盘、无遥测） | ✅ 通过（静态） | `ai_cache` 与页面代码无 `localStorage`/文件写入/上传/遥测调用（grep 零命中）；结果只存内存 |

### 2.1 功能清单（spec §3.1）核对

| ID | 功能 | 结论 |
|----|------|------|
| F1 | 工具入口 | ✅ |
| F2 | 自动检测 | ✅ |
| F3 | 占用统计 | ✅（A 类字节、小计、总量均有；与 du 偏差 0.34%） |
| F4 | 分级展示（B 类只读 + 打开目录） | ✅ 代码核对（B 类无勾选框、展示名称/路径） |
| F5 | B 类体积**按需计算** | ❌ 未实现（同 AC-06 / D2） |
| F6 | 确认后清理（列路径 + 合计） | ✅ dry-run 预览 + 弹窗逐条路径 |
| F7 | 结果反馈（成功/失败明细 + 重扫） | ✅ |
| F8 | 安全校验（白名单/规范化/链接/根/越界） | ✅（但入参形态与 spec R1 不符 → D3） |
| F9 | 异常隔离 | ✅ |
| F10 | 运行中保护（默认不勾选） | ✅ |
| F11 | 空态/错误态 | ⚠️ 后端 ✅，UI 未真机验证 |
| F12 | 四类 `cargo test` | ✅ existing 36 + 新增 13 全绿 |

---

## 3. 缺陷与偏差清单

> 结论口径：**后端已全部按 spec 的安全硬规则实现并通过实测**；以下问题分"需裁决"、"需修"、"文档/低危"三档。

### D1【高 · 待产品裁决】spec 与 plan 对 `paste-cache` / `usage-data` 的分类冲突

- 复现：
  1. 夹具 `~/.claude/paste-cache/a.bin`；
  2. `clean_ai_caches([{toolId:"claude-code", category:"cache", path:"<root>/paste-cache"}])`。
- 预期（spec §4.3）：`paste-cache/`、`usage-data/` 属"只读展示，绝不清理"，不应出现在可清理白名单。
- 实际：`catalog.rs:466-482` 将 `paste-cache`、`usage-data` 列入 `cache` 类别，实测 `deleted_count=1`，目录被永久删除。
- 依据：spec §4.2.1 A 类清单**不含**这两项，且 §4.3 明确列入"绝不触碰"；plan §5.3.1 却把它们标为 `cache ✅`（官方文档称可自动清理）。
- 影响：若以 spec 为验收基准，属于"删除规格外数据"；实测中 ac07/ac13 用例正是通过删除 `paste-cache` 完成。
- 建议：Product/Chief 二选一——(a) 实现按 spec 撤回这两项（推荐，风险最小）；(b) 修订 spec §4.3 并在文档中说明口径变更。**拍板前本报告按 spec 判该项为偏差**。

### D2【中 · 需修】F5 / AC-06 未实现：B 类体积在主扫描中递归统计

- 证据：`mod.rs` 的 `AiToolScanDto` 直接返回 `display_only_bytes`；`scan.rs:178-233` 对 **所有类别**（含 `sessions`、`model_cache`）执行 `expand_targets` + `measure_with`；`lib.rs` 的 `generate_handler!` 只有 3 个命令，无 `measure_tool_data`；前端无「计算占用」按钮（grep `计算占用|measure` 零命中）。
- 预期（spec F5/AC-06）：B 类默认只显示名称，用户点「计算占用」后才递归统计（≤10 s），不阻塞主扫描。
- 实际：主扫描同步遍历 B 类目录。夹具 100 MB 仅 1 ms 看不出问题，但真实机器上 Ollama `models/`（数十 GB）、Cursor/Windsurf `User/workspaceStorage`（常见 10~40 GB）会被纳入主扫描，**AC-05 的 5 s 预算有超时风险**，也与 spec "避免拖慢主扫描"的意图相反。
- 建议：拆出 `measure_tool_data(tool_id)` 命令 + 前端「计算占用」；或至少从主扫描移除 `model_cache`/`sessions` 的字节统计。

### D3【中 · 需确认】IPC 契约：前端回传绝对路径，违反 spec R1

- 证据：`mod.rs:158-165` `AiCacheCleanTarget { tool_id, category, path }`；`clean.rs:60-119` 把前端 `path` 当"候选"再按白名单精确匹配；`page.tsx:319-337` 把扫描返回的绝对路径回传给后端。
- 预期（spec R1/§6、design §6）：前端**只能传** `toolId + cacheId`，**永远不能传路径**；路径由后端白名单决定。
- 实际：传路径，但后端做了白名单精确匹配 + `canonicalize` 复核（实测拒绝全部伪造/越界目标）。**风险已被兜底压住，但契约本身不符合规格**。
- 建议：改为 `cacheId`（后端映射相对路径），或由 Chief 记录"允许传 path + 后端复核"的裁决并同步 spec/design。

### D4【中 · 功能缺口】spec 要求的路径未实现：Claude MCP 日志 / macOS 系统缓存根

- 证据：`catalog.rs` 中 claude-code 只有 `~/.claude`（含 `CLAUDE_CONFIG_DIR`）一个根；`grep claude-cli-nodejs|syscache` **0 命中**。Cursor/Windsurf 也只配了 `Application Support`，没有 macOS `~/Library/Caches/Cursor`、`~/Library/Caches/com.windsurf...` 根。
- 预期（spec §2.1 / §4.2.1 / §4.2.3 / §4.2.4）：Claude Code 的 `claude.mcp-logs`（macOS `~/Library/Caches/claude-cli-nodejs/`，spec 称"MCP 日志长期占用大头"）与 Cursor/Windsurf 的 `syscache` 应可清理。
- 实际：缺失，用户清理容量与 spec 预期有差距（功能缺失，不是误删风险）。
- 建议：补 catalog 根 + 白名单条目 + 单测（plan 已预留数据驱动扩展点）。

### D5【低 · 待裁决】工具矩阵超出 spec 范围

- 实际实现 9 个工具（spec v1 为 6 个）。其中：
  - **Claude Desktop**：spec §3.3 第 6 条明确"不动 Claude Desktop 缓存"，实现把它纳入并列出可清理项；
  - **VS Code 版 Copilot 扩展**：spec §2.1 写明 v1 只处理 `~/.copilot`（CLI），实现却扫描 `Code/User/globalStorage/github.copilot`；
  - **Ollama**：spec §1.4 非目标说不管模型权重；实现仅展示 `models`（`model_cache` 不可清理），风险可接受。
- 建议：Chief 确认是否接受扩容；若不接受，按 D5 从 catalog 移除或在 spec 中登记。

### D6【低】AC-02 排序：后端不排序，仅前端按可清理体积降序

- 证据：`scan.rs:305-320` 按 catalog 顺序 `collect`；`page.tsx:84-89` 前端排序。
- 影响：UI 满足 AC-02；但 IPC 返回本身不保证顺序，其它消费方（或直接断言 IPC 的测试）会不满足。
- 建议：文档注明"顺序由前端决定"，或后端排序。

### D7【低 · 信息】`ROOT_PROTECTED` 分支不可达，根目录被白名单先拒

- 证据：传工具根/主目录时返回 `PATH_NOT_ALLOWED`（实测），而非 `ROOT_PROTECTED`；因为根目录不可能精确匹配"根 + 相对白名单路径"。
- 影响：保护效果成立（拒绝 + 零变化），只是错误码语义不准确、`ROOT_PROTECTED` 成为死代码。
- 建议：把根/主目录/文件系统根判定前移到白名单匹配之前（或增加单测断言错误码）。

### D8【信息】与 plan 的决策差异：sessions 不清理 + 永久删除

- plan D1 建议 `sessions` 可清理（默认不勾 + riskAck）、D3 建议默认回收站；实现只清理 `cache`/`logs` 且**永久删除**。
- 评价：更保守，**与 spec 一致**（spec 明确"直接永久删除"、B 类不清理），不判为缺陷；建议同步更新 plan，避免后续评审误判。

### D9【低】AC-17 文案差异

- spec 要求无权限目录显示「无权限读取」；实现返回的 warning 是「部分内容无法读取（N 项）：<路径>」，并在卡片展示。语义等价、文案不同；建议统一文案或在 spec 放宽。

---

## 4. 无法验证项及原因

| 项 | 原因 |
|---|---|
| AC-01 / AC-06 / AC-07 / AC-17 的**真机 UI 表现**（导航可见性、确认弹窗焦点、扫描骨架、空态、无权限行） | 本机无法运行 `npm run tauri:dev`（需编译 Tauri 应用并开窗），只做了静态导出构建 + 代码核对；建议由人或有 GUI 的机器补 7 状态截图验收 |
| AC-05 的**真实 5 GB / 冷缓存**场景 | 无法在本机安全构造 5 GB 真实缓存；仅用 100 MB 夹具验证了算法与量级 |
| AC-18 断网真机 | 未断网执行；仅静态确认代码无网络/提权路径 |
| Windows / Linux 平台行为（大小写不敏感、junction、`%APPDATA%`） | 本机只有 macOS；相关逻辑有代码但未实测 |
| `~/Library/Caches/claude-cli-nodejs/`、Cursor/Windsurf 真实目录 | 本机无真实安装；同时发现该路径**未实现**（D4） |

---

## 5. 质量门与回归

| 检查 | 结果 | 证据 |
|---|---|---|
| `cargo fmt --check` | ✅ exit 0 | 无 diff（Coder 代码 + 新增测试均已格式化） |
| `cargo clippy --all-targets -- -D warnings` | ✅ exit 0 | `Finished dev profile`，0 warning |
| `cargo test`（全量） | ✅ | lib 36 passed、集成 13 passed、0 failed |
| `npm run lint` | ✅ exit 0 | 无告警 |
| `npm run build` | ✅ exit 0 | 静态页含 `/tools/ai-cache-clean`（其余 8 个路由正常） |

### 回归建议（修复 D1/D2/D3 后）

1. D1 若撤回清单：更新 `catalog.rs` 白名单 → 跑 `cargo test`（既有 `catalog_cleanable_paths_do_not_nest_protected` 会兜底）+ 本报告 ac13 用例；
2. D2 若新增 `measure_tool_data`：补「主扫描不遍历 B 类」的性能断言（可用 10 万个假文件夹具测主扫描耗时）；
3. D3 若改 `cacheId`：需同步改 `AiCacheCleanTarget`、`validate_target`、前端 `tauri.ts`/`page.tsx`，并重跑本报告 ac10/ac11 全量拒绝用例；
4. 每次修复后执行：
   ```bash
   cargo fmt --manifest-path src-tauri/Cargo.toml --check
   cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
   cargo test  --manifest-path src-tauri/Cargo.toml
   cargo test  --manifest-path src-tauri/Cargo.toml --test qa_ai_cache -- --nocapture --test-threads=1
   npm run lint && npm run build
   ```

---

## 附录 A：自动化测试清单（`src-tauri/tests/qa_ai_cache.rs`）

| 测试 | 覆盖 AC | 断言要点 |
|---|---|---|
| `ac04_scan_bytes_match_du_within_5_percent` | AC-04 | 逐项 + 合计 `du -sk` 偏差 ≤5%；白名单外 5 MB 不计 |
| `ac07_dry_run_defaults_true_and_deletes_nothing` | AC-07 | JSON 缺省 `dryRun=true`；目录树 SHA-256 前后一致；显式 false 才删 |
| `ac13_bc_untouched_by_clean` | AC-13 | 9 个 B/C 文件（含凭据/配置/未知目录）文件数 + SHA-256 不变；A 类被删 |
| `ac10_11_16_reject_forged_and_out_of_scope_targets` | AC-10/11/16 | 11 类非法目标拒绝 + 命令级限制（未知工具/空/65 项）；磁盘零变化 |
| `ac12_symlink_target_not_followed_or_deleted` | AC-12 | 链接项 `Skipped`、目标保留、内部链接目标不计数 |
| `ac19_clean_is_idempotent` | AC-19 | 第二次 0 释放、跳过而非失败、新增 B 类文件不受影响 |
| `ac15_partial_failure_is_isolated` | AC-15 | 只读目录项失败、其它成功、失败项无半删 |
| `ac17_empty_state_does_not_crash` | AC-17 | 全空/空目录 env、9 工具、0 字节、无 panic；缺失目标 skipped |
| `ac02_ac03_detection_and_env_override` | AC-02/03 | detected 恰好 2 个；`CLAUDE_CONFIG_DIR` 覆盖根；清理只作用覆盖目录 |
| `ac14_in_use_flag_from_mtime` | AC-14 | 新文件 in_use=true；旧 mtime=false |
| `ac08_ac09_freed_bytes_and_rescan` | AC-08/09 | freed=清理前字节；重扫归零 |
| `ac05_scan_is_fast_on_fixture` | AC-05 | 800 文件 / 100 MB 全量扫描 <5 s |
| `catalog_command_shape_and_real_env` | 契约 | catalogVersion/tools 数、env 覆盖真实生效、protected 含凭据/配置 |

## 附录 B：关键原始输出（节选）

```
AC-04 合计 scan=2090000B du=2097152B 偏差=0.34%（阈值 5%）
AC-07 缺省 dryRun 反序列化结果 dryRun=true
AC-07 dry_run=true freed=100 deleted=0 statuses=[WouldDelete]
AC-10 拒绝 伪造 cacheId/工具 id：claude.projects -> 命令级错误 TOOL_NOT_FOUND: 未知工具 id：claude.projects
AC-10 拒绝 ../ 越界路径                    -> status=Failed error=PATH_NOT_ALLOWED: 路径不允许包含 ..
AC-10 拒绝 把工具根目录当目标               -> status=Failed error=PATH_NOT_ALLOWED: 路径不在白名单目录内
AC-10 拒绝 凭据文件                        -> status=Failed error=PATH_NOT_ALLOWED: 路径不在白名单目录内
AC-10 超限错误：INVALID_ARGUMENT: 单次清理最多 64 项
AC-12 清理结果 deleted=1 skipped=1 statuses=[(…/paste-cache, Skipped, Some("PATH_IS_SYMLINK: 已跳过符号链接，不删除链接目标")), (…/image-cache, Deleted, None)]
AC-13 B/C 类文件数 before=9 after=9 含 SHA-256 的文件数=9
AC-15 失败原因：DELETE_FAILED: 删除失败：Permission denied (os error 13)
AC-17 空环境 tools=9 detected=[] cleanable=0 displayOnly=0
AC-19 第二次 freed=0 deleted=0 skipped=1 failed=0 error=Some("PATH_NOT_FOUND: 目标已不存在")
test result: ok. 13 passed; 0 failed; 0 ignored (qa_ai_cache)
test result: ok. 36 passed; 0 failed; 0 ignored (lib)
```

## 附录 C：质量门输出

```
$ cargo fmt --manifest-path src-tauri/Cargo.toml --check
[exit 0]

$ cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s)
[exit 0]

$ cargo test --manifest-path src-tauri/Cargo.toml
test result: ok. 36 passed  (lib)
test result: ok. 13 passed  (qa_ai_cache)
[exit 0]

$ npm run lint        # exit 0，无告警
$ npm run build       # exit 0
┌ ○ /tools/ai-cache-clean   (静态页生成成功)
```

## 附录 D：工具链问题（给团队，非代码缺陷）

- 本机 `cargo` 直连 crates.io 会长时间卡在 `Updating crates.io index`（`curl` 访问同一地址正常），并发执行多个 `cargo` 时还会因 `.package-cache` 文件锁形成互等（多个 bot 的 clippy/test 同时卡住）。
- 本次验收的绕过方式：隔离 `CARGO_HOME` + 配置 rsproxy 稀疏镜像 + 隔离 `CARGO_TARGET_DIR`（不修改仓库与 `.ops` 全局配置）：
  ```toml
  [source.crates-io]
  replace-with = "rsproxy-sparse"
  [source.rsproxy-sparse]
  registry = "sparse+https://rsproxy.cn/index/"
  ```
- 建议 Ops 把该镜像写进 `.ops/env.sh` 的 `CARGO_HOME/config.toml`，避免后续所有 bot 再次互相锁死。

---

**签署**：Tester · 2026-10-31
**结论**：安全硬规则与必测 7/8 项通过；AC-06 未实现、D1 分类冲突待裁决、D3 契约偏差待确认。建议 Coder 先修 D2/D3 并确认 D1，由其重跑回归后再进入 senior-qa / architect 评审。
