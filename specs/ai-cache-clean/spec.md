---
feature: ai-cache-clean
status: draft
owner: 产品（Product）
created: 2026-10-31
version: 1.3
updated: 2026-10-31
target: v1（MVP）
---

# AI 智能体缓存清理 · 功能规格（v1）

> 一句话：自动检测本机已安装的 AI 编码工具，列出**可安全清理的缓存/日志目录**及其占用，让用户一键释放磁盘空间，同时明确展示"会话记录、配置、凭据"等**永不删除**的数据。

## 版本记录

**v1.3（2026-10-31，D4 终裁）**
- **D4 延后到 v1.1**：macOS 系统缓存根（`~/Library/Caches/claude-cli-nodejs` 等）**不进 v1**；R15 / AC-23 及精确条目设计移入 §12「v1.1 backlog」，标注"未实现，留待 v1.1"。
- v1 工具矩阵、白名单与验收标准回到不含系统缓存根（9 工具不变，AC-01~22）；D5 维持。
- 理由：代码已稳定全绿，SystemCache 根是新增的删除路径，临交付前加风险不划算。

**v1.2（2026-10-31，D4 / D5 裁决回写）**
- D4：macOS 系统缓存目录纳入 `Cache` 类可清理范围，但只按**精确白名单条目**删除（Claude Code `<proj>/--mcp-logs-*`；Cursor / Windsurf `~/Library/Caches/<app>` 下指定的缓存子目录），禁止整根或 `*/*` 通配删除。（**已被 v1.3 取代：D4 延后到 v1.1**）
- D5：v1.1 的 **9 工具矩阵保持不变**；Claude Desktop 的 `claude_desktop_config.json`、扩展安装目录、config/state 文件归入「仅展示 / 受保护」，不进入可清理类别。

**v1.1（2026-10-31，回写实现与裁决）**
- R1-R3：路径改为白名单**精确匹配**；新增目录/文件两类目标语义（`logs_*.sqlite` 只删文件本身，绝不递归删同名目录）。
- Sessions（会话记录）与 ModelCache（模型权重，含 Ollama `models`）只展示占用，不提供清理入口；覆盖全部工具。
- 删除方式：v1 只有永久删除一种，不引入回收站（理由见 §10-R2）。
- 新增 R9 / AC-21：工具根经 env override 指向用户主目录、主目录上级、文件系统根时，该工具降级为「仅检测」，前端不可勾选。
- 新增 R10 / AC-22：后端单次上限 64 项，前端必须分批提交并合并结果，用户不应看到超限错误。
- 扫描「取消」改为「停止等待」：不承诺中断后端扫描。
- 工具矩阵对齐实现：9 个工具（补 Claude Desktop、Copilot VS Code 扩展、Ollama）；白名单表与 `src-tauri/src/ai_cache/catalog.rs` 一一对应。

**v1.0（2026-10-31，初稿）**
- 首次成稿。

---

## 1. 背景、目标用户与场景

### 1.1 目标用户

- 日常使用 AI 编码工具的**个人开发者**（macOS 为主，Windows 次之，Linux 尽力支持）。
- 已在用 Claude Code / Codex CLI / Cursor / Windsurf 中的 1~3 个，且**不愿意手动 `du` + `rm`**。
- 会看目录路径，但没有精力逐个确认"哪个能删"。

### 1.2 核心场景

| # | 场景 | 用户诉求 | 成功的样子 |
|---|------|----------|-----------|
| S1 | 系统盘告急，macOS「系统数据」/ Windows C 盘飘红，找不到空间去哪了 | 知道是哪个 AI 工具占的、能删的是什么 | 30 秒内看到"可清理 X GB"，一键清掉纯缓存 |
| S2 | 长期用 Claude Code，`~/.claude` 涨到十几 GB | 清日志/调试残留，**不丢会话记录** | 清掉 debug/日志/旧备份，`projects/` 原样保留 |
| S3 | 多工具并用，想统一比较占用 | 横向对比 + 批量清 | 卡片列出各工具可清理量，勾选后一次性清理 |
| S4 | 临时腾空间（装系统更新 / 大项目），不想动配置 | 可控、可回看、不误删 | 确认框逐条列出将删路径，清完显示实际释放量 |

### 1.3 目标（可量化）

1. 打开页面后**无需任何操作**自动完成检测与扫描，5 秒内给出可清理总量（本机条件见 AC-05）。
2. 清理动作**只作用于白名单内精确匹配的目标**；清理前后会话记录/配置/凭据的字节级内容零变化（AC-13）。
3. 一次清理的成功率 ≥95%（目录不存在/符号链接/占用 = 正常跳过，不算失败）。

### 1.4 非目标

- 不是通用磁盘清理器（不碰 Xcode、npm、Docker、系统缓存）。
- 不管理模型权重（Ollama / LM Studio / HuggingFace 见 §9）。
- 不做"迁移/搬家"，那是已有的「工具数据迁移」。

---

## 2. 范围与工具覆盖

### 2.1 v1 工具矩阵

| # | 工具 | 检测根目录 | 平台 | v1 能力 |
|---|------|-----------|------|---------|
| 1 | Claude Code | `~/.claude`（可用 `CLAUDE_CONFIG_DIR` 覆盖） | 三平台 | 清理缓存/日志 + 只读展示会话记录 |
| 2 | Claude Desktop | macOS `~/Library/Application Support/Claude`；Windows `%APPDATA%\Claude`；Linux `~/.config/Claude` | 三平台 | 清理 Electron 缓存/日志 + 只读展示会话状态 |
| 3 | Codex CLI | `~/.codex`（可用 `CODEX_HOME` 覆盖） | 三平台 | 清理缓存/日志（含 `logs_*.sqlite` 文件） + 只读展示会话记录 |
| 4 | Cursor | macOS `~/Library/Application Support/Cursor`；Windows `%APPDATA%\Cursor`；Linux `~/.config/Cursor` | 三平台 | 清理 Chromium 缓存/日志 + 只读展示工作区历史 |
| 5 | GitHub Copilot（VS Code 扩展） | 各编辑器 `.../User/globalStorage`：`Code`、`Code - Insiders`、`VSCodium` | 三平台 | 清理扩展缓存 + 只读展示扩展会话数据 |
| 6 | GitHub Copilot CLI | `~/.copilot` | 三平台 | 清理缓存/日志 + 只读展示会话状态 |
| 7 | Windsurf | macOS `~/Library/Application Support/Windsurf`；Windows `%APPDATA%\Windsurf`；Linux `~/.config/Windsurf` | 三平台 | 清理 Chromium 缓存/日志 + 只读展示工作区历史 |
| 8 | Gemini CLI | `~/.gemini`（可用 `GEMINI_CLI_HOME` 覆盖） | 三平台 | **只检测 + 只展示，不提供清理**（无 A 类白名单） |
| 9 | Ollama | `~/.ollama` | 三平台 | 清理日志；**模型权重（`models`）只展示不清理** |

> 判定"已安装"：任一检测根存在即视为已检测。根存在但白名单目标全不存在 → 仍显示卡片，标注「已检测到 · 暂无可清理缓存」。
> Claude Desktop 按 Electron 标准布局固化子目录，发布前用 AC-04 体积核对做真机复核。

### 2.2 数据分级（本规格的核心模型）

| 级别 | 定义 | 产品行为 |
|------|------|----------|
| **A 类：可清理缓存** | 删除后**无用户可见内容损失**，工具下次使用会按需重建（缓存、日志、调试残留、旧版本遗留目录、安装包副本）；含目录类与文件类目标 | 可勾选、可清理，默认勾选 |
| **B 类：只读数据** | 会话记录、编辑历史、工作区状态、模型权重（`Sessions` / `ModelCache` 类别），删除会造成**不可逆的信息损失** | 只在界面展示（名称 + 占用）+ 「打开目录」，**无勾选框、无清理入口** |
| **C 类：绝不触碰** | 凭据、账号、配置、插件/扩展、技能、记忆、状态库、根目录本身、符号链接目标、白名单外的一切 | 不展示为可清理项；后端拒绝任何删除请求（R12 兜底） |

**一句话规则：白名单之外的一切都不删；白名单内只删两个类别——`Cache`（缓存）与 `Logs`（日志）；目录删整个目录，文件（如 `logs_*.sqlite`）只删文件本身。**

### 2.3 检测规则

1. 根目录 = 环境变量优先（`CLAUDE_CONFIG_DIR`、`CODEX_HOME`、`GEMINI_CLI_HOME`）→ 否则按平台默认路径；界面不允许自定义路径。
2. 平台路径必须由 `$HOME` / `%USERPROFILE%` / `%APPDATA%` / `%LOCALAPPDATA%` / `XDG_*` 等系统变量推导，**禁止硬编码用户名**。
3. 不读注册表、不调用外部 CLI、不联网。
4. 工具清单必须**数据驱动**（工具表 + 白名单表），新增工具只需加配置与测试，不改界面逻辑。
5. **env override 降级**：override 指向「用户主目录本身 / 用户主目录的上级 / 文件系统根」→ 该工具整体降级为「仅检测」（`cleanable=false`），前端不出勾选框，后端拒绝其清理请求（见 R9、AC-21）。
6. 无 A 类白名单的工具（Gemini CLI）与 `model_cache` 类别：只展示占用，不提供清理入口。

---

## 3. MVP 功能清单

### 3.1 v1 必须（P0，缺一不可上线）

| ID | 功能 | 说明 |
|----|------|------|
| F1 | 工具入口 | 路由 `/tools/ai-cache-clean`，注册进工具导航（所有平台可见，非 Windows 专属） |
| F2 | 自动检测 | 进入页面即扫描全部 9 个工具根（见 §2.1），存在的才展示 |
| F3 | 占用统计 | 展示每个工具 A 类（缓存/日志）各项字节数、各工具小计、全局"可清理总量"；B 类占用另计 |
| F4 | 分级展示 | A 类可勾选（默认勾选，含一句话说明）；Sessions / ModelCache 只读展示，**不提供清理入口** |
| F5 | 按需计算 B 类体积 | B 类默认只显示名称，点击「计算占用」后再递归统计（避免拖慢主扫描） |
| F6 | 确认后清理 | 勾选 → 点击「清理选中项」→ **dry-run 预览** → 确认弹窗逐条列路径与合计 → 确认后才真正删除 |
| F7 | 结果反馈 | 展示成功/失败明细、实际释放量；自动重扫刷新数字 |
| F8 | 安全校验 | 后端白名单 + 路径规范化 + 符号链接/根目录/越界拒绝（见 §4.1、§6） |
| F9 | 异常隔离 | 单项失败不影响其他项；无权限/占用/不存在都给中性提示 |
| F10 | 运行中保护 | 目录内 10 分钟内有修改 → 标「可能正在使用」，默认不勾选 |
| F11 | 空态/错误态 | 无工具、全部无缓存、读取失败 均有明确文案 |
| F12 | 测试 | 白名单拒绝、路径精确匹配、符号链接拒绝、B/C 类零影响、单项失败隔离、override 降级 六类 `cargo test` |
| F13 | 分批提交 | 选中项 >64 时按 64 分批串行提交（预览与真删都分批），合并结果；用户不应看到"超过上限"错误 |
| F14 | 停止等待 | 扫描中可「停止等待」（前端停止展示/等待），不承诺中断后端扫描；文案不得写"已取消扫描" |

### 3.2 后续版本（明确不在 v1，别顺手做）

| 版本 | 内容 | 触发条件 |
|------|------|----------|
| v1.1 | macOS 系统缓存根清理（D4，设计见 §12） | 真机实测路径与体积后再启用 |
| v1.2 | 定时扫描提醒 / 阈值告警（>N GB 提示） | 不做后台常驻，仅打开页面时提示 |
| v2 | 新增工具：Cline / Roo Code / Continue / Zed / Trae | 路径固化进白名单 + 测试 |
| v2 | 模型权重管理（Ollama / HuggingFace 的下载与删除） | 独立需求：涉及重新下载成本，需单独确认流程 |

> 会话记录（`Sessions`）与模型文件（`ModelCache`）在 v1 及可预见版本都只展示、不提供清理入口；若将来要清理，必须单独出规格并由用户显式确认。

### 3.3 不做什么（v1 硬性排除）

1. **不做任意路径清理**：不提供"自定义目录""手动输入路径""全盘搜索大文件"。
2. **不做跨目录通配/正则删除**：唯一允许的是白名单模板**末段的单层文件名模式**（如 `logs_*.sqlite`），不允许 `**`、不允许匹配子目录内文件。
3. **不做用户自选的文件级删除**：只删白名单条目的完整目标——目录类目标删整个目录，文件类目标只删该文件本身，**绝不递归删除同名目录**。
4. **不删整个工具根目录**（`~/.claude`、`~/.codex`、`~/.cursor` 等永不整体删除）。
5. **不提供会话记录与模型文件的清理入口**：`Sessions`（含 Cursor/Windsurf `workspaceStorage`、Codex `sessions`、Claude `projects`）与 `ModelCache`（Ollama `models`）只展示占用。
6. **不清理配置、凭据、插件、扩展、记忆、状态库**（见 §4.3）。
7. **不做回收站模式**：v1 只有永久删除一种方式（理由见 §10-R2）。
8. **不做自动清理、开机自启、后台常驻**。
9. **不联网、不上传路径或体积数据、不设遥测**。
10. **不要求管理员权限**，不触发 UAC/提权。
11. **不做删除撤销**（永久删除；恢复靠工具重建缓存，文案必须写清楚）。
12. **不承诺中断后端扫描**：扫描只能"停止等待"（见 §5.3）。

---

## 4. 目录白名单与绝不触碰清单（写死）

### 4.1 全局硬规则（后端必须逐条实现）

| 规则 | 内容 |
|------|------|
| R1 | 白名单**硬编码在后端**（Rust 常量表，`src-tauri/src/ai_cache/catalog.rs`）。前端提交 `{toolId, category, path}`，其中 `path` 只是候选——**后端必须逐项按白名单模板重新校验**，不信任前端。 |
| R2 | **精确匹配**：候选路径必须命中「工具根 + 白名单相对路径」的解析结果（含末段单层 glob），且规范化后仍在工具根之下；含 `..`、越界、未知工具/类别、未命中模板 → 拒绝该项并记录原因。 |
| R3 | **目标语义**：只删除白名单内精确匹配的目标；目录类目标删整个目录；文件类目标（如 `logs_*.sqlite`）只删文件本身，**绝不递归删除同名目录**。 |
| R4 | 目标为**符号链接 / junction / reparse point** → 跳过（不跟随、不删目标），结果标注"已跳过：符号链接"。 |
| R5 | 绝不删除：工具根目录本身、磁盘根、用户主目录、系统目录（`/etc`、`/usr`、`C:\Windows`、`%ProgramFiles%`）。 |
| R6 | 目标不存在 → 跳过（视为幂等），不报错、不计入失败。 |
| R7 | 目标内最近 **10 分钟**内有文件修改 → 标记「可能正在使用」，默认不勾选（可手动勾选）。 |
| R8 | 逐项独立执行；单项失败（权限/占用/被删）只影响该项，其余继续；结果分别列出成功/跳过/失败原因。 |
| R9 | **根目录校验与降级**：工具根经 env override 指向「用户主目录本身 / 用户主目录的上级 / 文件系统根」→ 该工具 `cleanable=false`，降级为仅检测；前端不可勾选，后端拒绝其清理请求（`TOOL_DOWNGRADED`）。 |
| R10 | 单次清理请求最多 64 项；**前端必须按 64 分批串行提交**（预览与真删都分批）并合并结果，用户不应看到因超限导致的失败（AC-22）。 |
| R11 | 白名单模板只允许**末段单层通配**（如 `logs_*.sqlite`）；不支持 `**`、不支持正则。 |
| R12 | **全局兜底黑名单**：即使误写进白名单，以下 basename 也永不删除——`.credentials.json`、`auth.json`、`oauth_creds.json`、`credentials.json`、`config.json`、`config.toml`、`settings.json`、`settings.local.json`、`.claude.json`、`state.vscdb`、`state.vscdb.backup`、`history.jsonl`、`.env`、`id_ed25519`、`*.key`、`*.pem` 等（`FORBIDDEN_BASENAME`）。 |
| R13 | 不写任何持久化记录（不落盘历史/日志/数据库）；清理结果仅在页面内存中展示。 |
| R14 | 全程无网络请求；删除不用 shell/PowerShell，统一 Rust 原生 `fs`；`dry_run` 预览不得产生任何文件系统变更。 |

### 4.2 各工具白名单

> 本节与 `src-tauri/src/ai_cache/catalog.rs` 的 `TOOLS` 表**一一对应**：改其中一张表必须同步改另一张并补测试。
> **A 类（可清理）= `Cache` / `Logs` 类别**；**B 类（只展示，无清理入口）= `Sessions` / `ModelCache` 类别**。

#### 4.2.1 Claude Code

根：`$CLAUDE_CONFIG_DIR`（若设置）否则 `~/.claude`

| 类别 | 白名单条目（相对根） | 删除影响 |
|------|---------------------|---------|
| A · 缓存 | `cache`、`shell-snapshots`、`paste-cache`、`image-cache`、`todos`、`statsig`、`logs`、`plans`、`session-env`、`backups`、`usage-data` | 可自动重建；`paste-cache` 丢历史粘贴内容、`usage-data` 丢 `/insights` 历史报告，会话记录不受影响 |
| A · 日志 | `debug` | 失去调试日志 |
| B · 会话 | `projects`、`file-history`、`tasks`、`uploads`、`dev-mods`、`feedback`、`feedback-bundles`、`history.jsonl` | 会话 transcript、检查点、任务、上传/反馈内容，不可恢复 → 只展示 |

C 类（protected，绝不进入白名单）：`.credentials.json`、`settings.json`、`settings.local.json`、`plugins`、`skills`、`agents`、`commands`、`rules`、`output-styles`、`workflows`、`themes`、`keybindings.json`、`agent-memory`、`jobs`、`daemon`、`CLAUDE.md`、`remote-settings.json`、`policy-limits.json`、`stats-cache.json`。

#### 4.2.2 Claude Desktop

根：macOS `~/Library/Application Support/Claude`；Windows `%APPDATA%\Claude`；Linux `~/.config/Claude`

| 类别 | 白名单条目 | 删除影响 |
|------|-----------|---------|
| A · 缓存 | `Cache`、`Code Cache`、`CachedData`、`GPUCache`、`DawnGraphiteCache`、`DawnWebGPUCache`、`GrShaderCache`、`ShaderCache`、`Service Worker/CacheStorage`、`Service Worker/ScriptCache` | Electron 可重建缓存 |
| A · 日志 | `Crashpad`、`logs` | 失去诊断日志 |
| B · 会话 | `IndexedDB`、`Local Storage` | 应用内状态与会话，不可恢复 → 只展示 |

C 类（仅展示 / 受保护，不进入可清理类别）：`claude_desktop_config.json`、`extensions`（扩展安装目录）、`config.json` 与 `state*`（配置与状态文件）、`Preferences`、`Cookies`、`Network`。子目录按 Electron 标准布局固化，发布前用真机体积核对（AC-04）。

> D5：Claude Desktop 只清理 Electron 缓存/日志（上表 A 类）；配置、扩展安装目录、config/state 文件只展示占用、不提供清理入口。

#### 4.2.3 Codex CLI

根：`$CODEX_HOME`（若设置）否则 `~/.codex`

| 类别 | 白名单条目 | 删除影响 |
|------|-----------|---------|
| A · 缓存 | `cache`、`.tmp`、`tmp`、`plugins/cache`、`ambient-suggestions` | 可重建；`tmp` / `.tmp` 内有进行中任务时按 R7 默认不勾选 |
| A · 日志 | `logs_*.sqlite`（**文件类**，根级单层匹配） | 只删匹配到的日志数据库文件本身，不删同名目录、不递归 |
| B · 会话 | `sessions`、`archived_sessions`、`session_index.jsonl`、`history.jsonl` | 会话 rollout 与历史，不可恢复 → 只展示 |

C 类：`auth.json`、`config.toml`、`AGENTS.md`、`state_*.sqlite`、`goals_*.sqlite`、`sqlite`、`skills`、`rules`、`browser`、`computer-use`、`pets`、`.sandbox-bin`、`.sandbox-secrets`、`.codex-global-state.json`。

#### 4.2.4 Cursor

根：macOS `~/Library/Application Support/Cursor`；Windows `%APPDATA%\Cursor`；Linux `~/.config/Cursor`

| 类别 | 白名单条目 | 删除影响 |
|------|-----------|---------|
| A · 缓存 | `Cache`、`Code Cache`、`CachedData`、`GPUCache`、`DawnGraphiteCache`、`DawnWebGPUCache`、`GrShaderCache`、`ShaderCache`、`CachedExtensionVSIXs`、`CachedProfilesData`、`Service Worker/CacheStorage`、`Service Worker/ScriptCache` | 可重建；首次启动略慢 |
| A · 日志 | `Crashpad`、`logs` | 失去诊断日志 |
| B · 会话 | `User/History`、`User/workspaceStorage`（`.pack` / `.idx` / `state.vscdb`） | 工作区编辑历史与检查点，体积最大头（常见 10–40 GB），不可恢复 → 只展示 |

C 类：`User/globalStorage`、`User/settings.json`、`User/keybindings.json`、`extensions`。

#### 4.2.5 GitHub Copilot（VS Code 扩展）

根（分别检测）：`.../User/globalStorage`，其中 `...` 为 `Code`、`Code - Insiders`、`VSCodium`（macOS 在 `~/Library/Application Support`，Windows 在 `%APPDATA%`，Linux 在 `~/.config`）

| 类别 | 白名单条目 | 删除影响 |
|------|-----------|---------|
| A · 缓存 | `github.copilot` | 扩展缓存，可重建 |
| B · 会话 | `github.copilot-chat` | 扩展会话数据，不可恢复 → 只展示 |

C 类：`state.vscdb`、`storage.json`。

#### 4.2.6 GitHub Copilot CLI

根：`~/.copilot`

| 类别 | 白名单条目 | 删除影响 |
|------|-----------|---------|
| A · 缓存 | `pkg`、`cache` | 可重建 |
| A · 日志 | `logs` | 失去运行日志 |
| B · 会话 | `session-state`、`history`、`history-session-state` | 会话与命令历史，不可恢复 → 只展示 |

C 类：`config.json`、`mcp-config.json`。

#### 4.2.7 Windsurf

根：macOS `~/Library/Application Support/Windsurf`；Windows `%APPDATA%\Windsurf`；Linux `~/.config/Windsurf`

| 类别 | 白名单条目 | 删除影响 |
|------|-----------|---------|
| A · 缓存 | `Cache`、`Code Cache`、`CachedData`、`GPUCache`、`DawnGraphiteCache`、`DawnWebGPUCache`、`GrShaderCache`、`ShaderCache`、`CachedExtensionVSIXs`、`CachedProfilesData`、`Service Worker/CacheStorage`、`Service Worker/ScriptCache` | 可重建 |
| A · 日志 | `Crashpad`、`logs` | 失去诊断日志 |
| B · 会话 | `User/workspaceStorage` | 工作区状态与编辑历史，不可恢复 → 只展示 |

C 类：`User/globalStorage`、`User/settings.json`、`extensions`；另 `~/.codeium`（账号 token + Cascade 记忆）不在扫描范围内。

#### 4.2.8 Gemini CLI

根：`~/.gemini`（可用 `GEMINI_CLI_HOME` 覆盖）。**v1 无 A 类白名单，完全只读。**

| 类别 | 条目 | 说明 |
|------|------|------|
| B · 会话 | `tmp`、`history`、`shell_history` | 项目临时目录（含会话快照）、历史、shell 历史；属不可逆信息损失 → 只展示 |

C 类：`settings.json`、`oauth_creds.json`、`google_accounts.json`、`trustedFolders.json`、`projects.json`、`policies`、`extensions`、`skills`、`GEMINI.md`、`installation_id`、`user_id`。

#### 4.2.9 Ollama

根：`~/.ollama`

| 类别 | 条目 | 说明 |
|------|------|------|
| A · 日志 | `logs` | 运行日志，可删 |
| B · 模型 | `models` | 模型权重：删除后需重新下载（耗时/耗流量），**只展示、不提供清理入口** |

C 类：`id_ed25519`、`id_ed25519.pub`、`history`。

### 4.3 绝对不触碰清单（跨工具汇总）

**通用（任何情况下都不删）**

1. 工具根目录本身、磁盘根、用户主目录、系统目录、回收站目录。
2. 符号链接 / junction 指向的目标。
3. 未命中白名单模板的一切目标（**默认拒绝**，不是"默认允许再过滤"）。
4. 全局兜底黑名单 basename（R12：凭据、配置、状态库、密钥）。
5. 未列入 A 类的一切内容：`Sessions`（会话记录、编辑历史、工作区状态）与 `ModelCache`（模型权重）**只展示，不提供清理入口**。

**按工具**

| 工具 | 只读展示（B 类，无清理入口） | 绝不触碰（C 类 / protected） |
|------|------------------------------|------------------------------|
| Claude Code | `projects`、`file-history`、`tasks`、`uploads`、`dev-mods`、`feedback`、`feedback-bundles`、`history.jsonl` | `.credentials.json`、`settings.json`、`settings.local.json`、`plugins`、`skills`、`agents`、`commands`、`rules`、`output-styles`、`workflows`、`themes`、`keybindings.json`、`agent-memory`、`jobs`、`daemon`、`CLAUDE.md`、`remote-settings.json`、`policy-limits.json`、`stats-cache.json` |
| Claude Desktop | `IndexedDB`、`Local Storage` | `claude_desktop_config.json`、`extensions`（扩展安装目录）、`config.json` / `state*`（配置与状态）、`Preferences`、`Cookies`、`Network` |
| Codex CLI | `sessions`、`archived_sessions`、`session_index.jsonl`、`history.jsonl` | `auth.json`、`config.toml`、`AGENTS.md`、`state_*.sqlite`、`goals_*.sqlite`、`sqlite`、`skills`、`rules`、`browser`、`computer-use`、`pets`、`.sandbox-bin`、`.sandbox-secrets`、`.codex-global-state.json` |
| Cursor | `User/History`、`User/workspaceStorage` | `User/globalStorage`、`User/settings.json`、`User/keybindings.json`、`extensions` |
| Copilot（VS Code 扩展） | `github.copilot-chat` | `state.vscdb`、`storage.json` |
| Copilot CLI | `session-state`、`history`、`history-session-state` | `config.json`、`mcp-config.json` |
| Windsurf | `User/workspaceStorage` | `User/globalStorage`、`User/settings.json`、`extensions`、`~/.codeium`（不扫描） |
| Gemini CLI | `tmp`、`history`、`shell_history` | `settings.json`、`oauth_creds.json`、`google_accounts.json`、`trustedFolders.json`、`projects.json`、`policies`、`extensions`、`skills`、`GEMINI.md`、`installation_id`、`user_id` |
| Ollama | `models` | `id_ed25519`、`id_ed25519.pub`、`history` |

---

## 5. 交互与页面

### 5.1 信息架构（单页）

```
[ 顶部汇总卡 ]
  已检测 4 个工具 · 可清理 3.2 GB · 另有 8.7 GB 会话记录/模型文件（只读）
  [重新扫描]

[ 工具卡片 × N ]  （按可清理体积降序，无清理项的排后面）
  ┌ Claude Code                     可清理 2.1 GB ──────────┐
  │ [可能正在使用]                                          │
  │ 可清理缓存（勾选）                                      │
  │  ☑ debug/                    812 MB    调试日志 · 可重建 │
  │  ☐ shell-snapshots/          3 MB      可能正在使用      │
  │  ── 会话记录（只读，无清理入口）── 未计算 [计算占用] [打开目录] │
  │  projects/ · file-history/ · tasks/ …（仅名称，不可勾选）│
  └────────────────────────────────────────────────────────┘

[ 底部操作条（吸底）]
  已选 3 项 · 3.2 GB              [清理选中项]
```

### 5.2 主流程

1. **进入页面** → 自动扫描（骨架屏 + "正在扫描本机 AI 工具…"）→ 卡片逐个/整体出现。
2. **浏览** → 展开卡片查看每个 A 项路径、大小、一句话说明；B 类可「计算占用」（独立计算，不阻塞）。
3. **勾选** → A 类默认全选；吸底条实时显示已选数量与合计。
4. **清理** → 点击「清理选中项」→ 先做 **dry-run 预览**（只校验与预估，不改文件系统）：
   - 标题「确认清理」，逐条列出**绝对路径** + 大小（超过 20 条时折叠"另有 N 项"）；
   - 警示文案：「这些缓存删除后**不可恢复**，工具会在下次使用时自动重建；会话记录、配置与账号不受影响。」；
   - 按钮：「取消」（默认焦点）/「清理 N 项 · X GB」（危险色）；只有确认后才发真实清理请求。
5. **执行** → 按钮进入 loading + 计时；前端按 64 项分批串行提交并合并结果；全部结束后自动重扫。
6. **结果** → 顶部显示「已释放 X GB」；结果区列出成功/跳过/失败项（失败给原因与建议，如「请先退出 Cursor 后重试」）；数字刷新为清理后状态。

### 5.3 必须覆盖的状态

| 状态 | 文案/表现 |
|------|----------|
| 扫描中 | 骨架卡片 + 「正在扫描…」；页面可滚动、可切换导航 |
| 停止等待 | 扫描中点「停止等待」→ 前端停止等待并提示"已停止等待；后端扫描可能仍在后台完成，可稍后重新扫描"；**不得**写"已取消后端扫描" |
| 无任何受支持工具 | 空态：「未检测到 AI 编码工具」+ 支持工具清单（9 个名称） |
| 检测到但无可清理项 | 卡片仍显示，标注「已检测到 · 暂无可清理缓存」，B 类可查看 |
| 工具被降级 | `cleanable=false` 的卡片显示警告「根目录指向敏感位置，已降级为仅检测」；无勾选框、吸底条不计入其目标 |
| 单项读取无权限 | 该行标「无权限读取」，其余正常 |
| 清理执行中 | 吸底按钮禁用 + 计时器；卡片不可勾选；分批进度按"已完成 / 总数"累加 |
| 清理部分失败 | 结果区分「已清理」/「已跳过」/「未完成」，失败项给出原因 |
| 全部成功 | 「已释放 X GB」+ 空态文案「目前没有可清理的缓存」 |

### 5.4 视觉与可用性要求（遵循项目 frontend-design 规范）

- 沿用现有视觉语言：分层背景、`var(--card)` / `var(--accent)` 语义色、圆角与现有页面一致；**不新造主题**。
- 语义色：可清理（中性/绿）、可能正在使用（黄）、B 类只读（灰）、危险操作（红/强调色）。
- 每个 A 项必须有一句中文说明（"这是什么、删了会怎样"），不能只给路径。
- 危险按钮不可作为默认焦点；确认弹窗支持 `Esc` 取消。
- 窄窗口（≤640px）卡片单列、吸底条保持可见；键盘可达。
- 动效仅用于扫描骨架与卡片出现（≤200ms），不做花哨动画。

---

## 6. IPC 契约（安全关键，接口形态为约束）

```rust
// 1) 首屏 catalog：只做路径拼接 + exists 判断，不扫体积
get_ai_cache_catalog() -> AiCacheCatalog
// { platform, catalog_version, tools: [{ id, name, vendor, roots: [path], detected,
//   cleanable, warnings, protected }] }

// 2) 扫描：对已检测工具统计体积
scan_ai_cache_usage(request { tool_ids: Vec<String>, include_heavy: bool }) -> AiCacheScanResult
// { total_cleanable_bytes, total_display_only_bytes, duration_ms,
//   tools: [{ id, roots: [{ path, categories: [{ category, cleanable, bytes,
//     paths: [{ path, bytes, last_modified_ms, in_use, unreadable }] }] }] }] }
// 事件：ai-cache-scan-progress { tool_id, current_path, scanned_entries, tool_index, tool_count }

// 3) 清理：dry_run=true 只校验与预估，不改文件系统
clean_ai_caches(request {
  targets: [{ tool_id, category, path }],   // path 来自扫描结果；后端重新校验
  dry_run: bool,
}) -> AiCacheCleanResult
// { dry_run, freed_bytes, deleted_count, skipped_count, failed_count,
//   items: [{ tool_id, category, path, bytes, status }], warnings }
// 事件：ai-cache-clean-progress { tool_id, path, deleted_count, freed_bytes }
```

**实现约束**

- 前端提交的 `path` 只作候选：后端按 `tool_id + category` 找到白名单模板并**精确匹配**；未命中 → 该项失败，绝不"就近删除"。
- `Sessions` / `ModelCache` 类别一律拒绝清理（`CATEGORY_NOT_CLEANABLE`）；降级工具一律拒绝（`TOOL_DOWNGRADED`）；全局兜底黑名单拒绝（`FORBIDDEN_BASENAME`）。
- 目录类目标 `fs::remove_dir_all`；文件类目标 `fs::remove_file`，**不递归、不删同名目录**。
- 删除前紧邻一次 `symlink_metadata` 复查（不 follow），压缩扫描→清理之间的 TOCTOU 窗口。
- 单次请求 ≤64 项；前端按 64 分批串行提交（预览与真删都分批）并合并结果，不把超限错误暴露给用户。
- 删除不用 shell/PowerShell，统一 Rust 原生 `fs`；`dry_run` 不产生任何文件系统变更。
- 扫描与清理用 `async` + 阻塞任务 + 进度事件，不阻塞 WebView。
- 复用 `open_path` 打开目录（仅对扫描返回的路径）。

---

## 7. 验收标准（可测）


> 验证环境：macOS（主）与 Windows 各一台；测试夹具 = 用临时目录模拟各工具根（Rust 单测）+ 真机抽查。

| ID | 验收项 | 通过条件 / 验证方法 |
|----|--------|-------------------|
| AC-01 | 工具入口 | `/tools/ai-cache-clean` 出现在顶部导航与工具列表（macOS 与 Windows 均可见，不受 windowsOnly 限制）；访问后进入「最近使用工具」 |
| AC-02 | 自动检测 | 用夹具分别创建/删除 9 个工具根后刷新：列表**恰好**包含根存在的工具，按可清理体积降序；未安装的不出现 |
| AC-03 | 环境变量根 | `CLAUDE_CONFIG_DIR=/tmp/fixture-claude`（内含 `debug/`）时，检测根切到该目录，清理只作用于其中白名单项 |
| AC-04 | 体积准确性 | 每个 A 项目标显示字节数 = 目标内文件字节数之和；与 `du -sk`（Windows 用等效工具）偏差 ≤5%（以字节求和为准） |
| AC-05 | 扫描性能 | 9 工具、A 类合计 <5 GB、SSD 条件下，全量扫描 ≤5 s；扫描期间页面可滚动、可切换导航，不出现"未响应" |
| AC-06 | 分级展示 | `Sessions` / `ModelCache` 无勾选框、无清理入口；「计算占用」在 ≤10 s 内给出大小（大目录显示计算中）；不影响 A 类已显示的数字 |
| AC-07 | 预览 + 确认 | 未确认前磁盘零变化；点击清理先执行 `dry_run` 预览（不产生文件系统变更），弹窗逐条列**绝对路径** + 合计 + "不可恢复"警示，默认焦点在「取消」 |
| AC-08 | 释放量 | `freed_bytes` = 各成功项清理前字节数之和（误差 ≤1%）；顶部显示"已释放 X GB" |
| AC-09 | 清理后重扫 | 成功项目标不存在（目录/文件两类都验证）；卡片可清理量归零（允许 ≤1 MB 立即重建）；汇总同步刷新 |
| AC-10 | 白名单拒绝 | 以 `Cache` 类别提交会话路径（`~/.claude/projects`、`~/.codex/sessions`）、`../etc`、空路径、未知工具/类别、`Sessions`/`ModelCache` 类别 → 全部失败且磁盘无变化（`cargo test`） |
| AC-11 | 路径越界拒绝 | 规范化后不在工具根内、含 `..`、被替换为指向 `/tmp` 的链接 → 拒绝并记录原因（`cargo test`，R2） |
| AC-12 | 符号链接 | 目标为 symlink/junction：不删除、不跟随，结果标注"已跳过：符号链接"，链接目标仍存在（**目录与文件两类**都测） |
| AC-13 | B/C 类零影响 | 清理前后对 B/C 类目录与文件做文件数 + SHA-256 比对完全一致（夹具 A/B/C 混合树逐项断言；含 Cursor `workspaceStorage`、Codex `sessions`、Ollama `models`） |
| AC-14 | 运行中保护 | A 项目标内写 mtime=now 的文件 → 显示「可能正在使用」且默认未勾选；手动勾选后仍可清理 |
| AC-15 | 单项失败隔离 | 构造必然失败项（父目录只读 / macOS `chflags uchg` 锁定文件 / Windows 独占打开）→ 该项失败并给中文原因，其他项成功；结果分列成功/失败（注：仅把目录设只读在 Unix 上未必失败，测试不能依赖它） |
| AC-16 | 根目录保护 | 任何情况下请求删除工具根（`~/.claude`、`~/.codex`、`~/.cursor`、`~/.gemini`、`~/.ollama`）都被拒绝（`cargo test`） |
| AC-17 | 空态/异常态 | 无工具 → 空态文案；无权限目录 → 「无权限读取」；两种情况都不白屏、不抛未捕获异常 |
| AC-18 | 无网络 / 无提权 | 断网下全部功能可用；无网络请求代码路径；普通权限完成所有操作，无 UAC/提权弹窗 |
| AC-19 | 幂等 | 连续两次清理同一批：第二次成功且 `freed_bytes = 0`，无错误 |
| AC-20 | 隐私 | 清理结果不落盘（无历史文件、无 localStorage 路径记录）；代码中无上传/遥测调用 |
| AC-21 | override 降级 | `CLAUDE_CONFIG_DIR=$HOME`、`CODEX_HOME=$HOME`、`CLAUDE_CONFIG_DIR=/`（及主目录上级）→ 该工具 `cleanable=false`：前端无勾选框、卡片显示降级原因、清理请求被 `TOOL_DOWNGRADED` 拒绝（`cargo test` + 手工验证） |
| AC-22 | 分批提交 | 构造 65 个待清理目标：前端按 64 分批（1 批 64 + 1 批 1）串行提交，全部执行并合并 `freed_bytes`/items；UI **不出现**"单次最多 64 项"之类错误 |

**质量门（沿用项目宪法）**：`npm run lint`、`cargo fmt --check`、`cargo clippy` 干净；`cargo test` 覆盖 AC-10/11/12/13/14/16/21；PR 需过 architect + senior-qa（+ staff-infrastructure 看 Windows 路径与 CI）。

---

## 8. 安全边界（逐条回答"删错怎么办"）

1. **清理前是否确认**：是，始终确认。确认弹窗内容来自 **dry-run 预览**（不产生任何文件系统变更），逐条列绝对路径与大小；默认焦点在「取消」，`Esc` 取消；无"跳过确认"设置。
2. **是否只删缓存类目录**：只删白名单精确匹配的目标，且仅限 `Cache` / `Logs` 两个 A 类类别；目录类删整个目录，**文件类（如 `logs_*.sqlite`）只删文件本身，绝不递归删除同名目录**；`Sessions` / `ModelCache` 只展示、不提供清理入口。
3. **会话记录/配置/凭据绝不删清单**：见 §4.3 —— Claude `projects`、`file-history`、`.credentials.json`；Codex `sessions`、`state_*.sqlite`、`auth.json`；Cursor / Windsurf `User/workspaceStorage`、`User/globalStorage`；Copilot CLI `history` 系列与 `config.json`；Gemini 全部子目录；Ollama `models`；外加 R12 全局兜底黑名单（凭据/配置/状态库/密钥 basename）。
4. **删除方式只有永久删除**：v1 不引入回收站——(a) 不为此新增依赖/系统 API；(b) 回收站不释放磁盘空间，与"腾空间"目标矛盾。文案必须明写"不可恢复，工具会按需重建"。
5. **误删防线（五层）**：①后端白名单精确匹配（前端路径不被信任）②路径规范化 + 必须在工具根内 ③符号链接 / 根目录 / 禁用 basename / 降级工具一律拒绝 ④dry-run 预览 ⑤确认弹窗逐条列路径。任一层不通过即拒绝执行。
6. **env override 防护**：override 指向用户主目录、主目录上级、文件系统根 → 工具降级为「仅检测」，前端不可勾选（R9、AC-21）。
7. **权限模型**：不需管理员权限；权限不足只标记该项失败，不尝试提权。
8. **并发安全**：删除前紧邻复查目标属性；若扫描后被替换为符号链接 → 跳过。
9. **隐私**：只做元数据遍历统计大小，不读取文件内容；不联网、不落盘。

---

## 9. 非功能要求

| 项 | 要求 |
|----|------|
| 平台 | macOS 12+（Apple Silicon / Intel）、Windows 10+；Linux 提供相同路径逻辑（尽力支持，Windows/macOS 为验收平台） |
| 性能 | 扫描 ≤5 s（AC-05）；清理 1 GB ≤10 s；单次操作期间 UI 不冻结 |
| 内存 | 扫描峰值不随文件数线性增长（只累加大小，不保留树） |
| 可靠性 | 单项失败不影响整批；目录被删/占用/无权限均不崩溃 |
| 可维护性 | 工具与白名单数据驱动；新增工具 = 配置 + 测试，不改 UI 逻辑 |
| 语言 | 中文 UI（沿用项目现状），错误信息为可读中文 |
| 合规 | 遵循 `.specify/memory/constitution.md`：TS strict、Rust 无 `unwrap` 于用户输入、能力最小化、无用 PowerShell |

---

## 10. 裁决记录（已闭合，无待决策项）

| # | 议题 | 裁决 | 理由 |
|---|------|------|------|
| R1 | Cursor / Windsurf `workspaceStorage`（`.pack` 编辑历史） | **不清理，只展示**（实现为 `Sessions` 类别） | 属编辑历史与检查点，删除不可逆；体积大不等于可删 |
| R2 | 删除方式 | **v1 只有永久删除一种，不引入回收站** | 不新增回收站依赖/系统 API；回收站不释放空间，与目标矛盾 |
| R3 | 会话记录（`Sessions` 类别，全工具） | **只展示占用，不提供清理入口** | 比"可勾选但默认不选"更严：从交互层消除误删可能 |
| R4 | Ollama `models`（模型权重，`ModelCache` 类别） | **只展示，不提供清理入口** | 重新下载成本高（时间/流量），不属于"可自动重建" |
| R5 | macOS 系统缓存目录（`~/Library/Caches/claude-cli-nodejs` 等） | **不进 v1，留 backlog（见 §12），v1.1 再做**；届时按精确白名单条目删除，不整根删、不做通配 | 代码已稳定全绿，SystemCache 根是新增的删除路径，临交付前加风险不划算 |
| R6 | 工具矩阵与 Claude Desktop 数据边界 | **保持 v1.1 的 9 工具矩阵**；Claude Desktop 的 `claude_desktop_config.json`、扩展安装目录、config/state 文件**仅展示 / 受保护**，不进入可清理类别 | 配置与扩展删后不可恢复；9 工具的覆盖范围已验证，不因未启用的系统缓存根而扩表 |

> 以上为已拍板结论。若后续要改（例如清理 `workspaceStorage`），需新开规格并按"会话清理"单独设计确认流程，不在 v1 范围内。

---

## 11. 交付与工作范围粗估

- 分支：`feat/ai-cache-clean` → PR 到 `main`；提交遵循 Conventional Commits + 中文摘要。
- 改动面：`src-tauri/src/lib.rs`（2 个新命令 + 白名单表 + 测试）、`src/lib/tools.ts`（注册入口）、`src/app/tools/ai-cache-clean/page.tsx`（新页面）、`src/lib/tauri.ts`（封装）。
- 粗估：Rust 1.5 人日、前端 1.5 人日、测试/联调 1 人日（不含评审与修复）。实现已落地；本规格 v1.1 为对实现与裁决的回写，后续改动以「先改规格 + 测试」为准。
- 建议顺序：白名单 + 扫描（含测试）→ 页面 → 清理 + 确认 → 结果态与真机验证。

---

## 12. v1.1 backlog（D4 系统缓存根，未实现，留待 v1.1）

> **终裁（v1.3）**：D4 不进 v1。代码已稳定全绿，SystemCache 根是新增的删除路径，临交付前加风险不划算；v1.1 再做。以下设计原样保留，v1 不做任何实现承诺，也不进入 AC-01~22。

### 12.1 设计约束（原 R15；v1.1 实施时生效）

- 概念：**系统缓存根** = macOS `~/Library/Caches/<app>`、`~/Library/Logs/<app>` 这类由系统约定存放缓存的根；与工具主根（`~/.claude` 等）分开建模，独立路径根 + 独立白名单。
- 只允许**精确条目**：固定子路径，或「固定父目录 + 末段单层模式」（如 `<项目目录>/--mcp-logs-*`）。
- **禁止整根删除**，禁止 `*` / `*/*` 匹配根下任意子项；未列入白名单的子目录一律不动；根下无匹配条目时显示"无可清理项"，不退回整根删除。
- 校验沿用 v1 全部规则：候选路径必须命中条目模板且位于该系统缓存根之下，仍适用符号链接跳过（R4）、兜底黑名单（R12）、降级（R9）、单次 64 项分批（R10）。

### 12.2 候选条目（未经真机实测，不启用）

| 工具 | 系统缓存根 | 精确条目 | 说明 |
|------|-----------|---------|------|
| Claude Code | macOS `~/Library/Caches/claude-cli-nodejs` | `<项目目录>/--mcp-logs-*` | `<项目目录>` 必须来自该根下的实际磁盘枚举（单层路径段，不接受 `..` 或路径分隔符）；只删 `--mcp-logs-` 开头的子目录，不删项目目录、不删缓存根。Linux 对应 `~/.cache/claude-cli-nodejs`；Windows 未实测 |
| Claude Code | macOS `~/Library/Logs/Claude` | 该目录本身（目录类） | 旧版日志输出位置，整目录均为日志 |
| Cursor | macOS `~/Library/Caches/Cursor` | `Cache`、`Code Cache`、`CachedData`、`GPUCache`、`DawnGraphiteCache`、`DawnWebGPUCache`、`GrShaderCache`、`ShaderCache` | 只删精确子目录，不删根 |
| Windsurf | macOS `~/Library/Caches/com.exafunction.windsurf` | 同上（同组 Electron 缓存子目录名） | 只删精确子目录，不删根 |

### 12.3 验收标准（v1.1 启用时转为正式 AC）

- **B-AC-23（原 AC-23，系统缓存根精确条目）**：macOS 夹具：`~/Library/Caches/claude-cli-nodejs/<proj>/--mcp-logs-x/` 可清理，同根下 `<proj>/other/`、`<proj>/`、缓存根本身保留（清理前后 SHA-256 不变）；`~/Library/Caches/Cursor` 下只删白名单子目录，其它子目录与根保留；根下无匹配条目时显示"无可清理项"且**不执行整根删除**（`cargo test` + 手工）。

---

## 附录 A：路径依据

- Claude Code `~/.claude` 目录与保留策略（含"删了会怎样"表）：[Explore the .claude directory](https://code.claude.com/docs/en/claude-directory)（官方文档，`cleanupPeriodDays`、application data、state files to keep 等小节）
- Codex CLI 配置与 `CODEX_HOME`：[openai/codex docs/config.md](https://github.com/openai/codex/blob/main/docs/config.md)
- GitHub Copilot CLI 用户目录 `~/.copilot`（含 `lsp-config.json` 等）：[github/copilot-cli README](https://github.com/github/copilot-cli)
- Cursor / Windsurf 为 VS Code 派生（Electron），`Cache` / `CachedData` / `Code Cache` / `GPUCache` / `CachedExtensions` / `logs` 为其通用缓存目录；`User/workspaceStorage`、`User/globalStorage`、`~/.codeium` 为用户数据。
- Gemini CLI `~/.gemini/tmp` 承载项目哈希下的临时/会话快照数据 —— 因此 v1 归入 B 类。

> 白名单按本规格固化为常量。若某工具后续版本改名/新增缓存路径，**必须走规格变更**（追加路径 + 测试），禁止运行时"猜目录"或通配匹配。

## 附录 B：术语

- **A 类缓存**：删除后无用户可见内容损失、工具可自动重建的目标（目录或文件）。
- **B 类只读数据**：会话记录、编辑历史、模型权重，删除即信息损失，只展示不清理。
- **C 类**：凭据、账号、配置、插件扩展、记忆、根目录、白名单外一切，永不触碰。
- **可能正在使用**：目录内 10 分钟内有文件修改 → 该项默认不勾选。
