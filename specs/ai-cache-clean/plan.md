# AI 智能体缓存清理 — 技术方案（plan.md）

- 状态：已按实现回写（2026-10-31，对齐 Chief 裁决与已合并实现；QA 以本版为准）
- 特性分支：`feat/ai-cache-clean`（基于 `main`）
- 目标读者：Coder（实现）、Reviewer（设计/安全审查）、Tester（验收）
- 约束来源：`.specify/memory/constitution.md`、`CLAUDE.md`、`AGENTS.md`

---

## 1. 结论摘要（TL;DR）

新增一个独立工具页 `/tools/ai-cache-clean`，扫描本机已安装的 AI 编码工具，把其数据目录拆成
**可清理类别**（cache / logs）、**仅展示类别**（sessions / model_cache）和**受保护内容**（配置、凭据、
扩展、状态库）。支持按工具、按类别、按路径三级勾选清理，默认走 **dry-run**，实际执行为 **永久删除**
（v1 不提供回收站选项）。

实现分三块，全部落在既有技术边界内：

1. **Rust 新增模块 `src-tauri/src/ai_cache/`**：`mod.rs`（DTO + 3 个 Tauri command）+ `catalog.rs`（路径白名单常量与解析）
   + `scan.rs`（体积扫描）+ `clean.rs`（校验与删除）+ `tests.rs`（单元测试）。
2. **前端新增页面** `src/app/tools/ai-cache-clean/`，复用 `/tools` 布局与现有视觉语言；
   注册进 `src/lib/tools.ts`，IPC 封装进 `src/lib/tauri.ts`。
3. **零新增依赖**：`walkdir`、`rayon`、`globset`、`serde` 均已在 `Cargo.toml` 中；
   不新增回收站依赖（v1 只用 Rust 原生 `fs` 永久删除）；不新增 npm 包；不改 Tauri capabilities
   （自定义 command 不需要额外权限）；不改 `tauri.conf.json`。

关键安全立场：**前端永远只能提交"工具 id + 类别 + 绝对路径"，Rust 侧白名单重新解析并校验后才允许删除**；
删除集合是封闭白名单（`catalog.rs` 常量），不是文件名黑名单。

---

## 2. 现状与可复用资产

| 现状 | 位置 | 对本特性的意义 |
|---|---|---|
| Rust 单文件 `lib.rs`（2357 行） | `src-tauri/src/lib.rs` | 本特性必须拆独立模块，避免 `lib.rs` 继续膨胀 |
| 命令统一 `Result<_, String>`，错误为中文文案 | `lib.rs:230-…` | 沿用；新增错误采用 `CODE: 文案` 前缀（§6.5） |
| 磁盘扫描：`async` command + `spawn_blocking` + `rayon` + 进度事件 | `lib.rs:909-957`、`lib.rs:754-891` | **直接复用其思路**：重活 off-UI-thread、`par_iter` 并行、跳符号链接 |
| 现有 `delete_path` 走 `trash`（回收站） | `lib.rs:1061-1076` | **不复用**：按裁决本特性只做永久删除，不引入回收站路径（零新增依赖） |
| 路径校验范式：绝对路径 + 拒绝 `..` + 白名单前缀 | `lib.rs:1148-1191` | 本特性加严：canonicalize + 根目录前缀 + 符号链接拒绝 |
| 前端：`invokeTauri<T>` 薄封装 + 页面内 `listen<Event>` | `src/lib/tauri.ts`、`src/app/tools/disk-heatmap/page.tsx:562` | 沿用；新增事件监听与类型 |
| 工具注册表 | `src/lib/tools.ts` | 新增一条 no-`windowsOnly` 条目 |
| `tool-data-migrate` 会把 `~/.claude` 等目录**软链到其它盘** | `src-tauri/src/lib.rs:630-711`、`src/app/tools/tool-data-migrate/page.tsx` | 本特性必须处理"根目录是符号链接""类别目录是符号链接"两种情况（§7.3/§7.4） |

性能基线（来自 constitution）：任何操作不得阻塞 WebView 事件循环；本方案用 `spawn_blocking` + 事件节流保证。

---

## 3. 范围

### 3.1 v1 范围

- 9 个工具：Claude Code、Claude Desktop、Codex CLI、Cursor、GitHub Copilot（VS Code 扩展）、
  GitHub Copilot CLI、Windsurf、Gemini CLI、Ollama。
- macOS / Linux / Windows 三平台（按编译目标 `cfg!` 选择根目录，不做运行时 OS 插件依赖）。
- 系统缓存根（D4）**不进 v1**：v1 不扫描、不清理 macOS `~/Library/Caches`、`~/Library/Logs` 等系统缓存目录；
  相关设计已评审但未实现，留 v1.1 backlog（§16）。
- 工具矩阵固定 9 个（spec v1.1 / D5）：不新增工具条目；
  Claude Desktop 的 `claude_desktop_config.json`、扩展安装目录、config/state 文件仅展示/受保护。
- 存在性检测 + 体积扫描 + 按工具/类别/路径的选择 + dry-run + 永久删除 + 结果与失败明细。
- 仅清理 cache / logs 两类；sessions / model_cache 只展示占用（`cleanable=false`），不提供删除入口。
- 扫描进度事件（当前工具、当前路径、已扫条目数）。

### 3.2 明确不在 v1

- 逐文件时间线/火焰图可视化（有 `disk-heatmap`，不重复做）。
- 定时自动清理、后台常驻监控（无数据库，v1 不做持久化）。
- "一键卸载工具""清理注册表/环境变量"（越界，属于 `windows-migrate`）。
- LM Studio、Continue、OpenCode、Aider、Cline、Trae、Cursor CLI、Windsurf/Codeium CLI —— 路径待实测，v2 加表即可（catalog 数据驱动，加一个条目即可上线）。
- 扫描取消：v1 只做前端「停止显示结果」（后端任务仍会跑完，文案须诚实，见 §6.11）；真取消放 v2。

---

## 4. 数据模型

### 4.1 领域对象

```
Tool（AI 工具，如 claude-code）
 └─ Root（该工具的数据根目录，按平台解析；通常 1 个，VS Code 系可能 3 个）
     └─ Category（类别：cache / logs 可清理；sessions / model_cache 仅展示）
         └─ Target（具体相对路径，是目录或文件；绝对路径由 Root + 相对路径解析）
```

- **只有 Category 可清理（cache/logs）、Target 出现在 `catalog.rs` 白名单里，才允许删除。**
  根目录本身、根目录下未列出的任何内容（配置、凭据、扩展、SQLite 状态库……）一律不碰；
  sessions / model_cache 类别只展示占用，不提供删除入口（§4.2）。
- Tool 是否"已检测"= 任一 Root 存在。若 Root 存在但所有 Target 都不存在，UI 显示"已安装 · 无缓存"。
- 体积口径：只统计白名单 Target（目录递归、文件取 metadata.len）。**不返回整目录体积**，
  避免为了"总占用"多扫一遍未识别内容（Ollama 模型目录可达数十 GB，扫描代价与风险都不划算）。
  UI 文案用"已识别缓存占用"而不是"总占用"。
- 两段式扫描（控预算）：主扫描只递归统计可清理类别（cache / logs），sessions / model_cache 只返回路径、
  标 `measured=false`；用户点"计算占用"时带 `includeHeavy=true` 二次扫描才统计这两类，
  结果分别汇总到 `cleanableBytes` / `displayOnlyBytes`。

### 4.2 类别语义（固定 4 类）

| key | 中文 | 默认勾选 | 风险等级 | 语义 |
|---|---|---|---|---|
| `cache` | 缓存 | ✅（`cleanable=true`） | safe | 删了会自动重建：HTTP/GPU/代码缓存、索引、changelog、临时快照 |
| `logs` | 日志 | ✅（`cleanable=true`） | safe | 调试日志、崩溃转储、沙箱请求日志 |
| `sessions` | 会话记录 | ❌（`cleanable=false`，v1 仅展示） | history | 聊天/会话/回滚快照/文件历史；只展示占用，不提供删除入口 |
| `model_cache` | 模型文件 | ❌（`cleanable=false`，v1 仅展示） | redownload | 模型权重；只展示占用，不提供删除入口 |

受保护内容不建模为类别（不返回、不可传参），仅在 `catalog.protected` 里用于 UI 展示说明与测试断言。
`cleanable` 由后端 `AiCacheCategory::is_cleanable()` 唯一决定（只有 cache / logs 为 true），前端不自行推断。

---

## 5. 跨平台路径清单（catalog 数据源）

### 5.1 根目录基址解析

`catalog.rs` 只存"逻辑基址 + 相对子路径"，运行时展开。**同一工具在三平台的"相对子目录"基本一致，
只有根目录不同**，所以表 5.2 与 5.3 分开写（DRY），Coder 实现时就是两个常量数组的笛卡尔组合。

| `BaseDir` | macOS | Linux | Windows |
|---|---|---|---|
| `Home` | `$HOME` | `$HOME` | `%USERPROFILE%`（回退 `HOMEDRIVE+HOMEPATH`） |
| `MacApplicationSupport` | `$HOME/Library/Application Support` | — | — |
| `XdgConfig` | `~/.config`（mac 上部分跨平台 CLI 仍用它） | `$XDG_CONFIG_HOME` 或 `~/.config` | — |
| `XdgData` | `~/.local/share` | `$XDG_DATA_HOME` 或 `~/.local/share` | — |
| `XdgCache` | `~/.cache` | `$XDG_CACHE_HOME` 或 `~/.cache` | — |
| `AppDataRoaming` | — | — | `%APPDATA%` 或 `%USERPROFILE%\AppData\Roaming` |
| `LocalAppData` | — | — | `%LOCALAPPDATA%` 或 `%USERPROFILE%\AppData\Local` |

根级环境变量覆盖（**必须支持，否则工具数据被迁移到别的盘就检测不到**）：

| 工具 | 环境变量 | 说明 |
|---|---|---|
| Claude Code | `CLAUDE_CONFIG_DIR` | 设置后所有 `~/.claude/*` 改在此目录下（官方文档明确） |
| Codex CLI | `CODEX_HOME` | 同义覆盖（⚠️ 以实测版本为准，取不到就回退 `~/.codex`） |
| Gemini CLI | `GEMINI_CLI_HOME` | 同义覆盖（IRFlow 文档确认存在） |
| Ollama | `OLLAMA_MODELS` | ⚠️ v1 不支持重定位的模型目录（不猜测、不删）；检测不到就在 UI 提示 |

解析规则：
- `env_override` 存在且值为绝对路径 → 用它作为根；否则用 `base + relative`。
- 全部结果做**词法去重 + canonicalize 去重**（两个工具/多个编辑器根指向同一现实目录时只保留一个，
  防止重复计数与重复删除）。
- 根目录 canonicalize 失败（不存在/权限）→ Root 标记 `exists=false`，不是错误。

### 5.2 工具根目录总览

| # | id | 名称 | macOS | Linux | Windows |
|---|---|---|---|---|---|
| 1 | `claude-code` | Claude Code | `~/.claude`（或 `$CLAUDE_CONFIG_DIR`） | 同 macOS | `%USERPROFILE%\.claude` |
| 2 | `claude-desktop` | Claude Desktop | `~/Library/Application Support/Claude` | `~/.config/Claude` | `%APPDATA%\Claude` |
| 3 | `codex` | Codex CLI | `~/.codex`（或 `$CODEX_HOME`） | 同 macOS | `%USERPROFILE%\.codex` |
| 4 | `cursor` | Cursor | `~/Library/Application Support/Cursor` | `$XDG_CONFIG_HOME/Cursor` 或 `~/.config/Cursor` | `%APPDATA%\Cursor` |
| 5 | `copilot-vscode` | GitHub Copilot（VS Code 扩展） | `~/Library/Application Support/{Code,Code - Insiders,VSCodium}/User/globalStorage` | `~/.config/{Code,Code - Insiders,VSCodium}/User/globalStorage` | `%APPDATA%\{Code,Code - Insiders,VSCodium}\User\globalStorage` |
| 6 | `copilot-cli` | GitHub Copilot CLI | `~/.copilot` | `~/.copilot` | `%USERPROFILE%\.copilot` |
| 7 | `windsurf` | Windsurf | `~/Library/Application Support/Windsurf` | `~/.config/Windsurf`（或 `$XDG_CONFIG_HOME/Windsurf`） | `%APPDATA%\Windsurf` |
| 8 | `gemini-cli` | Gemini CLI | `~/.gemini`（或 `$GEMINI_CLI_HOME`） | 同 macOS | `%USERPROFILE%\.gemini` |
| 9 | `ollama` | Ollama | `~/.ollama` | `~/.ollama`（系统服务可能在 `/usr/share/ollama/.ollama`，v1 不处理） | `%USERPROFILE%\.ollama` |

> 图例：✅ 已确认；⚠️ 待实测（实现时用 `ls` 核对真实安装，确认后再进白名单，否则注释掉并在 `catalog.rs` 留 TODO）。

> **系统缓存根（D4）不进 v1**：`~/Library/Caches/claude-cli-nodejs`、`~/Library/Caches/Cursor`、
> `~/Library/Caches/com.exafunction.windsurf`、`~/Library/Logs/Claude`、`~/.cache/claude-cli-nodejs`
> 等系统缓存目录 v1 不扫描、不清理（代码中无相关条目）。已评审的设计与测试点移入 v1.1 backlog（§16），
> 工具矩阵仍固定 9 个（D5）。

### 5.3 各工具明细（相对根目录的路径 → 类别）

> 图例：`cache` / `logs` = 可清理；`sessions` / `model_cache` = **v1 仅展示、不提供清理**；
> 类型列 `dir` / `file` / `file glob` 同时是白名单模板类型（§7.8 只允许同类型命中）。

#### 5.3.1 Claude Code（`claude-code`）— 依据官方文档《Explore the .claude directory》✅

| 相对路径 | 类型 | 类别 | 说明 |
|---|---|---|---|
| `shell-snapshots` | dir | cache ✅ | shell 启动快照，干净退出即删，崩溃遗留 |
| `paste-cache` | dir | cache ✅ | 大段粘贴内容缓存 |
| `image-cache` | dir | cache ✅ | 旧版本粘贴图片缓存（≥2.1.274 已移到临时目录） |
| `todos` / `statsig` / `logs` | dir | cache ✅ | 旧版本遗留目录，官方称已不再写入 |
| `cache` | dir | cache ✅ | 如 `changelog.md`，后台自动刷新 |
| `debug` | dir | logs ✅ | `--debug` / `/debug` 的每会话日志 |
| `plans` | dir | cache ✅ | plan mode 草稿文件（官方：删除无用户可见损失） |
| `session-env` | dir | cache ✅ | 每会话环境元数据 |
| `backups` | dir | cache ✅ | `~/.claude.json` 的历史备份（最多 5 份，损坏时可救配置；风险自担） |
| `usage-data` | dir | cache ✅ | `/insights` 报告与缓存分析数据（官方列入自动清理目录） |
| `projects` | dir | sessions ❌ | 会话 transcript（含 system prompt/工具输出，**含明文敏感信息**）+ 每项目 auto memory |
| `file-history` | dir | sessions ❌ | 编辑前快照，删后历史会话的 checkpoint 回滚失效 |
| `tasks` | dir | sessions ❌ | 任务清单（可能仍有在用任务） |
| `uploads` | dir | sessions ❌ | Remote Control 会话附件 |
| `dev-mods` | dir | sessions ❌ | 会话中 Claude 写的 mod |
| `feedback` / `feedback-bundles` | dir | sessions ❌ | 未发送的反馈草稿/打包 |
| `history.jsonl` | file | sessions ❌ | 上/下键与 `Ctrl+R` 的提示词历史 |

受保护（绝不删除，仅展示）：`.credentials.json`（登录凭据）、`settings.json` / `settings.local.json`、
`plugins/`（含 `installed_plugins.json`）、`skills/`、`agents/`、`commands/`、`rules/`、`output-styles/`、
`workflows/`、`themes/`、`keybindings.json`、`agent-memory/`、`jobs/`、`daemon/`、`CLAUDE.md`、
`remote-settings.json`、`policy-limits.json`、`stats-cache.json`、以及根同级的 `~/.claude.json`。

> 系统缓存根（`~/Library/Caches/claude-cli-nodejs` 等）v1 不处理，见 §16 backlog。

#### 5.3.2 Claude Desktop（`claude-desktop`）— Electron 标准布局 ✅/⚠️

| 相对路径 | 类型 | 类别 | 说明 |
|---|---|---|---|
| `Cache` / `Code Cache` / `CachedData` / `GPUCache` / `DawnGraphiteCache` / `DawnWebGPUCache` / `GrShaderCache` / `ShaderCache` | dir | cache ✅ | Chromium 缓存 |
| `Service Worker/CacheStorage` / `Service Worker/ScriptCache` | dir | cache ✅ | 仅缓存 |
| `Crashpad` | dir | logs ✅ | 崩溃转储 |
| `logs` | dir | logs ✅ | 应用日志（mac 上日志也可能在 `~/Library/Logs/Claude`，属 v1.1 backlog，见 §16） |
| `IndexedDB` / `Local Storage` | dir | sessions ⚠️ | 本地会话/界面态；v1 仅展示，不提供清理 |

仅展示/受保护（C 类，**不进入可清理类别**，D5）：`claude_desktop_config.json`（MCP 配置）、
`extensions/`（扩展安装目录）、`config.json` 与 `state*`（配置与状态文件）、`Preferences`、`Cookies`、`Network`。
D5 口径：Claude Desktop **只清理上表 Electron 缓存/日志（A 类）**；配置、扩展安装目录、config/state 文件
只展示占用、不提供清理入口。

#### 5.3.3 Codex CLI（`codex`）— 目录随版本变化，标 ⚠️ 的必须实测

| 相对路径 | 类型 | 类别 | 说明 |
|---|---|---|---|
| `cache` | dir | cache ✅ | 应用层工具定义缓存 |
| `.tmp` / `tmp` | dir | cache ✅ | 插件克隆、marketplace 临时目录 |
| `plugins/cache` | dir | cache ✅ | 版本化插件包，可重新拉取 |
| `ambient-suggestions` | dir | cache ✅ | 项目环境建议缓存 |
| `logs_*.sqlite` | file glob | logs ✅ | 运行日志库（根级单层匹配；文件类，只删文件本身） |
| `sessions` / `archived_sessions` | dir | sessions ❌ | rollout 会话回放；v1 仅展示 |
| `session_index.jsonl` | file | sessions ❌ | 会话索引；v1 仅展示 |
| `history.jsonl` | file | sessions ❌ | 提示词历史；v1 仅展示 |

受保护：`auth.json`（API Key，**凭据**）、`config.toml`、`AGENTS.md`、`state_*.sqlite`、`goals_*.sqlite`、`sqlite/`、
`skills/`、`rules/`、`browser/`、`computer-use/`、`pets/`、`.sandbox-bin/`、`.sandbox-secrets/`、
`.codex-global-state.json`。
⚠️ v2 候选（当前版本未列入白名单，不清理）：`vendor_imports`、`node_repl`、`.sandbox`、
`computer-use-turn-ended`、`generated_images`。

#### 5.3.4 Cursor（`cursor`）— 根目录已由文档确认 ✅，子目录按 Chromium/VS Code 布局

| 相对路径 | 类别 | 说明 |
|---|---|---|
| `Cache` / `Code Cache` / `CachedData` / `GPUCache` / `DawnGraphiteCache` / `DawnWebGPUCache` / `GrShaderCache` / `ShaderCache` | cache ✅ | Chromium 缓存 |
| `CachedExtensionVSIXs` / `CachedProfilesData` | cache ✅ | 扩展安装包下载缓存 |
| `Service Worker/CacheStorage` / `Service Worker/ScriptCache` | cache ✅ | 缓存 |
| `Crashpad` | logs ✅ | 崩溃转储 |
| `logs` | logs ✅ | 应用日志 |
| `User/History` | sessions ⚠️ | 本地文件历史（VS Code 同构） |
| `User/workspaceStorage` | sessions ⚠️ | 每工作区状态（含对话关联数据，删除丢历史） |

受保护：`User/globalStorage/state.vscdb`（**聊天历史数据库**，官方解析工具明确从该库读取历史）、
`state.vscdb.backup`、`User/settings.json`、`User/keybindings.json`、`extensions/`、`User/globalStorage/storage.json`。
⚠️ v2 候选（待实测命名）：`User/globalStorage/anysphere.cursor-*`（向量索引/embeddings 缓存，通常很大）。

#### 5.3.5 GitHub Copilot — VS Code 扩展（`copilot-vscode`）

| 相对路径（相对 `.../User/globalStorage`） | 类别 | 说明 |
|---|---|---|
| `github.copilot` | cache ⚠️ | 扩展遥测/缓存数据（不承载体裁凭据；token 在系统密钥链） |
| `github.copilot-chat` | sessions ⚠️ | 聊天会话与检索索引，删除丢聊天历史且需重建索引 |

受保护：`state.vscdb` 等其它扩展数据（不在白名单，自动受保护）。

#### 5.3.6 GitHub Copilot CLI（`copilot-cli`）

| 相对路径（相对 `~/.copilot`） | 类别 | 说明 |
|---|---|---|
| `logs` | logs ✅ | 运行日志 |
| `pkg` | cache ⚠️ | 下载的 CLI 包，可重新下载 |
| `cache` | cache ⚠️ | 若存在 |
| `session-state` / `history` / `history-session-state` | sessions ⚠️ | 会话状态与提示词历史 |

受保护：`config.json`、`mcp-config.json`、以及 `~/.config/github-copilot/`（`hosts.json`/`config.json`，凭据相关，v1 不列为工具根）。

#### 5.3.7 Windsurf（`windsurf`）

| 相对路径 | 类别 | 说明 |
|---|---|---|
| `Cache` / `Code Cache` / `CachedData` / `GPUCache` / `DawnGraphiteCache` / `DawnWebGPUCache` / `GrShaderCache` / `ShaderCache` | cache ✅ | Chromium 缓存 |
| `Service Worker/CacheStorage` / `Service Worker/ScriptCache` | cache ✅ | 缓存 |
| `CachedExtensionVSIXs` / `CachedProfilesData` | cache ✅ | 扩展包缓存 |
| `Crashpad` | logs ✅ | 崩溃转储 |
| `logs` | logs ✅ | 应用日志 |
| `User/workspaceStorage` | sessions ⚠️ | 工作区状态（含 Codeium 会话关联） |

受保护：`User/globalStorage/state.vscdb`（聊天/状态库）、`User/settings.json`、`extensions/`、
`User/globalStorage/*`（Codeium 登录态等，白名单外自动受保护）；另 `~/.codeium`（账号 token + Cascade 记忆）
不在扫描范围内。

#### 5.3.8 Gemini CLI（`gemini-cli`）— v1 无 A 类白名单，**完全只读**

| 相对路径 | 类别 | 说明 |
|---|---|---|
| `tmp` | sessions ❌ | `tmp/<slug>/chats/*.jsonl` 会话、checkpoints、`logs.json`、`.project_root`；v1 仅展示 |
| `history` | sessions ❌ | 工作区注册/历史（`.project_root`）；v1 仅展示 |
| `shell_history` | sessions ❌ | shell 命令历史；v1 仅展示 |

受保护：`settings.json`、`oauth_creds.json`（**凭据**）、`google_accounts.json`、`trustedFolders.json`、
`projects.json`、`policies/`、`extensions/`、`skills/`、`GEMINI.md`、`installation_id`、`user_id`、MCP token 文件。
> Gemini CLI **不提供任何清理入口**（无 cache/logs 白名单），只检测 + 展示会话类占用。

#### 5.3.9 Ollama（`ollama`）

| 相对路径 | 类别 | 说明 |
|---|---|---|
| `logs` | logs ✅ | 服务日志（mac/Windows；Linux 常走 journald） |
| `models` | model_cache ❌ | 模型权重（`blobs` + `manifests`）；v1 仅展示占用，不提供清理入口 |

受保护：`id_ed25519` / `id_ed25519.pub`（Ollama 私钥/公钥）、`history`。
⚠️ 若设置了 `OLLAMA_MODELS`，v1 不识别该目录（宁可不清理，也不猜路径）。

### 5.4 全局硬性禁删（跨所有工具，作为最后一道黑名单兜底）

即使误写进 catalog，以下 basename 也必须在校验层被拒绝（并有单测断言 catalog 里没有它们）：

```
.credentials.json  auth.json  oauth_creds.json  credentials.json  hosts.json
config.json  config.toml  settings.json  settings.local.json  .claude.json
claude_desktop_config.json  state.vscdb  state.vscdb.backup  history.jsonl
*.key  *.pem  .env  id_ed25519
```

> 说明：`logs_*.sqlite` 属于日志文件、允许；`*sqlite` 不能一刀切禁（如 Codex `logs_2.sqlite` 是日志库）。

### 5.5 漂移与版本

- `catalog.rs` 顶部维护 `CATALOG_VERSION: u32 = 1`，随路径表变更递增；扫描结果带回该值，便于排查"某版本突然扫不到"。
- 每条 ⚠️ 条目在代码里留 `// verify: <来源/日期>` 注释；未实测的路径**先注释掉，不入白名单**。
- 未知新目录不会被删除（白名单机制天然安全），v2 通过"未识别项（只读、展示体积）"补齐发现能力。

---

## 6. Rust 设计

### 6.1 模块划分

```
src-tauri/src/
├── lib.rs                 # 只加：mod ai_cache; + generate_handler 里 3 个命令路径
└── ai_cache/
    ├── mod.rs             # 公开：DTO、请求/响应、3 个 command、事件结构；错误构造函数
    ├── catalog.rs         # 平台枚举、BaseDir 解析、ToolSpec 常量表、白名单匹配
    ├── scan.rs            # 体积测量、进度节流、并行扫描、scan_all_with_heavy
    ├── clean.rs           # 目标校验、去重、dry-run、永久删除、clean_all
    └── tests.rs           # #[cfg(test)] 单元测试（catalog + 校验 + 扫描 + 清理）
```

理由：`lib.rs` 已 2357 行且文件头是一大段 `use`；再塞 ~900 行会显著抬高 review 与编译心智成本。
不引入 `thiserror`/`anyhow`（保持零新依赖、沿用 `Result<T, String>` 与 `CODE: 文案` 约定）。

### 6.2 接口契约（完整类型）

```rust
// ai_cache/mod.rs
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const CATALOG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiCacheCategory { Cache, Logs, Sessions, ModelCache }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiRisk { Safe, History, Redownload }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiPathKind { Dir, File }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCachePathDto {
    pub path: String,          // 绝对路径（展示用，非 canonical 形式）
    pub bytes: u64,
    pub kind: AiPathKind,      // dir | file：文件类目标只能匹配文件（§7.8，B1）
    pub last_modified_ms: u64, // 最近修改时间（毫秒），用于展示"上次使用"
    pub in_use: bool,          // 最近 10 分钟内有修改 → 默认不勾选，允许手动勾选
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheCategoryDto {
    pub category: AiCacheCategory,
    pub label: String,          // 中文
    pub risk: AiRisk,
    pub cleanable: bool,        // false = 仅展示（sessions / model_cache），前端不渲染删除入口
    pub measured: bool,         // 体积是否已计算：主扫描只有 cache/logs 为 true
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
    pub detected: bool,          // 任一 root 存在
    pub cleanable: bool,         // false = 根目录降级为仅检测（§7.7），不可清理
    pub roots: Vec<AiCacheRootDto>,
    pub cleanable_bytes: u64,    // 仅 cache + logs
    pub display_only_bytes: u64, // 仅已计算的 sessions / model_cache
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheScanResult {
    pub platform: String,        // "macos" | "linux" | "windows"
    pub catalog_version: u32,
    pub scanned_at_secs: u64,
    pub duration_ms: u64,
    pub total_cleanable_bytes: u64,
    pub total_display_only_bytes: u64, // 已计算的 sessions / model_cache 合计
    pub tools: Vec<AiToolScanDto>,
    pub warnings: Vec<String>,
}

// 目录展示接口（页面首屏用，不扫体积，毫秒级返回）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolCatalogDto {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub roots: Vec<String>,      // 解析出的绝对路径（含不存在的）
    pub detected: bool,
    pub protected: Vec<String>,  // 受保护项（相对路径，仅展示）
    pub cleanable: bool,         // false = 根目录降级为仅检测（§7.7）
    pub warnings: Vec<String>,   // 降级原因
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
    #[serde(default)]
    pub tool_ids: Vec<String>,   // 空 = 全部
    #[serde(default)]
    pub include_heavy: bool,     // false（默认）= 主扫描只算 cache/logs；
                                 // true = 「计算占用」二次请求，递归统计 sessions / model_cache
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheCleanTarget {
    pub tool_id: String,
    pub category: AiCacheCategory,
    pub path: String,            // scan 返回的绝对路径；Rust 侧重新校验
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCacheCleanRequest {
    pub targets: Vec<AiCacheCleanTarget>, // 单次 ≤ MAX_TARGETS_PER_REQUEST（64），见 §6.10
    #[serde(default = "default_true")]
    pub dry_run: bool,           // 缺省 true：任何调用方忘了传都只会预览
}

fn default_true() -> bool { true }

/// 单次清理请求的目标数上限（对应 §6.10 的分批约束）。
pub const MAX_TARGETS_PER_REQUEST: usize = 64;

// ── 响应 ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiCleanStatus { WouldDelete, Deleted, Skipped, Failed }

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
```

### 6.3 命令签名（3 个）

```rust
#[tauri::command]
pub fn get_ai_cache_catalog() -> Result<AiCacheCatalog, String>;

#[tauri::command]
pub async fn scan_ai_cache_usage(
    window: tauri::Window,
    request: AiCacheScanRequest,
) -> Result<AiCacheScanResult, String>;

#[tauri::command]
pub async fn clean_ai_caches(
    window: tauri::Window,
    request: AiCacheCleanRequest,
) -> Result<AiCacheCleanResult, String>;
```

- scan / clean 都是 `async` + `tauri::async_runtime::spawn_blocking`，与 `scan_disk_usage`（`lib.rs:909`）同构；
  同步 command 在主线程执行，会让整个 WebView 冻结。
- `clean_ai_caches` 单次最多 64 项（`MAX_TARGETS_PER_REQUEST`），超出即命令级 `INVALID_ARGUMENT`；
  前端分批并合并结果（§6.10）。
- 注册：`lib.rs` 顶部 `mod ai_cache;`；`generate_handler![…]` 追加
  `ai_cache::get_ai_cache_catalog, ai_cache::scan_ai_cache_usage, ai_cache::clean_ai_caches`。
- 事件名（kebab-case，与 `disk-scan-progress` 一致）：`ai-cache-scan-progress`、`ai-cache-clean-progress`。
- **不改** `Cargo.toml`、`tauri.conf.json`、`capabilities/default.json`（自定义命令不受 fs scope 约束）。

### 6.4 前端调用参数形状（camelCase，Tauri 2 默认）

```ts
// 主扫描（只算 cache / logs；sessions / model_cache 标 measured=false）
await invokeTauri<AiCacheScanResult>("scan_ai_cache_usage", {
  request: { toolIds: [], includeHeavy: false },
});
// 用户点「计算占用」：二次扫描，把展示类别也统计出来
await invokeTauri<AiCacheScanResult>("scan_ai_cache_usage", {
  request: { toolIds: [], includeHeavy: true },
});
// 清理（先 dry-run；单次 ≤ 64 项，超出前端分批）
await invokeTauri<AiCacheCleanResult>("clean_ai_caches", {
  request: {
    targets: [{ toolId: "claude-code", category: "cache", path: "/Users/me/.claude/paste-cache" }],
    dryRun: true,
  },
});
```

### 6.5 错误约定

沿用项目 `Result<T, String>`；新增错误统一 `<CODE>: <中文文案>`，前端原样展示，必要时按 `:` 前缀做逻辑分支。

| CODE | 触发 | 层级 / 处理 |
|---|---|---|
| `INVALID_ARGUMENT` | targets 为空、单批 > 64 项 | 命令级 `Err` |
| `TOOL_NOT_FOUND` | `toolId` 不在 catalog | 命令级 `Err` |
| `CATEGORY_NOT_CLEANABLE` | 类别为 sessions / model_cache，或该工具无此类别 | 命令级 `Err`（不静默跳过） |
| `TOOL_DISABLED` | 工具已降级为仅检测（根目录指向 home / home 上级 / 文件系统根，§7.7） | 命令级整批 `Err`（保证"一个字节都不删"） |
| `PATH_NOT_ABSOLUTE` | 路径不是绝对路径 | **单条** `Failed` |
| `PATH_NOT_ALLOWED` | 含 `..`/`.`、不在白名单目录内、不在工具根内、命中凭据黑名单 | **单条** `Failed` |
| `PATH_IS_SYMLINK` | 目标是符号链接 / junction | **单条** `Skipped`（不删链接目标） |
| `PATH_NOT_FOUND` | 扫描后被删 / 被移动 | **单条** `Skipped`（幂等：第二次清理即此码） |
| `PATH_NOT_READABLE` | 权限不足无法读取元数据 | **单条** `Failed` |
| `ROOT_NOT_FOUND` / `ROOT_PROTECTED` | 根目录不可访问 / 目标等于根、home、文件系统根 | **单条** `Failed` |
| `DELETE_FAILED` | 系统拒绝删除（占用、只读、权限） | **单条** `Failed`（不重试、不降级） |
| `SCAN_FAILED` / `CLEAN_FAILED` | `spawn_blocking` join 失败（任务 panic） | 命令级 `Err` |

设计要点：**单条失败不打断整体**，用 `items[].status` + `warnings` 表达部分成功；命令级 `Err` 只用于
"请求本身非法"。Tauri 序列化 `Err(String)` 时前端 `catch` 拿到字符串，现有 `extractErrorMessage` 可直接用。

### 6.6 catalog 数据结构与路径解析（示意）

```rust
// catalog.rs
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Platform { MacOs, Linux, Windows }

pub fn current_platform() -> Platform { /* cfg! 分支 */ }

#[derive(Clone, Copy)]
pub enum BaseDir {
    Home, MacApplicationSupport, XdgConfig, XdgData, XdgCache,
    AppDataRoaming, LocalAppData,
}

pub struct RootSpec {
    pub platforms: &'static [Platform],
    pub base: BaseDir,
    pub relative: &'static str,            // 如 ".claude" / "Cursor/User/globalStorage"
    pub env_override: Option<&'static str>, // 如 Some("CLAUDE_CONFIG_DIR")
}

pub struct CategorySpec {
    pub category: AiCacheCategory,
    // label / risk / default_selected / description 由 category_meta(category) 统一提供（DRY）
    pub paths: &'static [&'static str],    // 相对路径；文件类目标允许末段含 '*'
}

pub struct ToolSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    pub roots: &'static [RootSpec],
    pub categories: &'static [CategorySpec],
    pub protected: &'static [&'static str],
}

pub fn tools() -> &'static [ToolSpec] { &TOOLS }
```

`catalog.rs` 中 Claude Code 条目的完整写法（其余工具按 §5.3 表照抄同一形状）：

```rust
const ALL_PLATFORMS: &[Platform] = &[Platform::MacOs, Platform::Linux, Platform::Windows];

const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        id: "claude-code",
        name: "Claude Code",
        vendor: "Anthropic",
        roots: &[RootSpec {
            platforms: ALL_PLATFORMS,
            base: BaseDir::Home,
            relative: ".claude",                  // Windows 由 Path::join 处理分隔符
            env_override: Some("CLAUDE_CONFIG_DIR"),
        }],
        categories: &[
            CategorySpec {
                category: AiCacheCategory::Cache,
                paths: &["cache", "shell-snapshots", "paste-cache", "image-cache",
                         "todos", "statsig", "logs", "plans", "session-env",
                         "backups", "usage-data"],
            },
            CategorySpec {
                category: AiCacheCategory::Logs,
                paths: &["debug"],
            },
            CategorySpec {
                category: AiCacheCategory::Sessions,   // 仅展示（is_cleanable=false）
                paths: &["projects", "file-history", "tasks", "uploads", "dev-mods",
                         "feedback", "feedback-bundles", "history.jsonl"],
            },
        ],
        protected: &[".credentials.json", "settings.json", "settings.local.json",
                     "plugins", "skills", "agents", "commands", "rules", "output-styles",
                     "workflows", "themes", "keybindings.json", "agent-memory",
                     "jobs", "daemon", "CLAUDE.md", "remote-settings.json",
                     "policy-limits.json", "stats-cache.json"],
    },
    // …其余 8 个工具同形状，数据以 §5.3 表为准（v1 不含系统缓存根，见 §16）
];
```

解析规则：

1. `env_override` 有值且绝对 → 直接用（仍走 canonicalize 校验）；否则 `resolve_base(base)?.join(relative)`。
   **env 读取必须可注入**：`fn resolve_roots_with(spec, platform, lookup: &dyn Fn(&str) -> Option<OsString>)`，
   生产传 `|k| env::var_os(k)`，单测传假查找器（避免测试里 `set_var` 污染并行测试）。
2. 词法 + canonicalize 去重（跨工具/跨编辑器根）。
3. **目标路径解析器**：`root.join(rel)`，若 `rel` 末段含 `*` → `read_dir(parent)` 按 `globset::Glob` 匹配
   **根级文件名**（不接受含 `/` 的通配，杜绝跨目录模式）；文件类模板只收 `is_file()` 的命中项，
   目录类模板只收 `is_dir()` 的命中项（B1，见 §7.8）。
4. 白名单匹配（clean 校验用）：解析后的绝对路径与"工具 × 类别 × 实际存在的目标"逐一相等比较
   （Windows 下路径大小写不敏感比较，见 §7.3）。不做"前缀宽松匹配"；模板类型（dir/file）不符同样拒绝。

> 系统缓存根（D4）所需的 `RootKind::SystemCache`、根级 `overrides`、`children_globs` 设计属
> v1.1 backlog，见 §16；v1 的 `catalog.rs` 不包含这些结构。

### 6.7 扫描实现（`scan.rs`）

```
scan_all_with_heavy(platform, lookup, tool_ids, include_heavy, sink):
  start = Instant::now()
  specs = 选中的 ToolSpec（空 = 全部；未知 id → 命令级 Err(TOOL_NOT_FOUND)，不静默忽略）
  results = specs.par_iter().map(|spec| scan_tool(spec, include_heavy)).collect()
  // rayon 的 IndexedParallelIterator 保序，前端按 catalog 顺序展示
  汇总 total_cleanable_bytes / total_display_only_bytes / warnings，返回
```

`scan_tool`（对每个 resolved root）：
1. `root.exists()` 为 false → `exists=false`，categories 全空，跳过。
2. 工具级 `policy.cleanable=false`（降级，§7.7）→ 正常返回数据，但 `cleanable=false` + warnings。
3. 对每个 CategorySpec（v1 所有根的类别条目来自 `ToolSpec.categories`）：
   - `measured = include_heavy || category.is_cleanable()`；未测的展示类别只展开路径、`bytes=0`、`measured=false`。
   - 解析实际存在的 targets（固定条目 / 末段 glob；`symlink_metadata` 判断，
     符号链接 → warning，不 follow）；不存在的 target 不返回。
   - 每个 target 调 `measure_bytes()`：目录 `WalkDir::follow_links(false)` 跳过 symlink 条目，
     文件取 `metadata.len()`；同时得出 `last_modified_ms`；`in_use = 最近 10 分钟内有修改`
     （`IN_USE_WINDOW_MS = 10 * 60 * 1000`）。
4. 每处理一个条目 `maybe_emit` 一次，**节流 200ms** 触发进度事件（§6.8）。

性能：
- 不构建树、不返回逐文件数据——只返回 target 级聚合，payload 几十~几百条，杜绝 disk-heatmap 式前端卡顿。
- 工具级 `par_iter`，单工具内部顺序 walk（避免磁盘随机 IO 抖动 + 进度路径有意义）；
  rayon 默认线程数即可，不额外建池。
- 主扫描**不递归** sessions / model_cache（大目录留到 `includeHeavy=true` 二次请求），保证首屏扫描预算。
- `WalkDir.follow_links(false)` 防循环；读失败（权限）只计 `unreadable` 并 warning，不中断。

### 6.8 进度节流

```rust
struct ProgressCtx {
    window: tauri::Window,
    tool_index: usize,
    tool_count: usize,
    tool_id: &'static str,
    scanned: AtomicU64,
    last_emit_ms: AtomicU64,   // epoch millis
}

fn maybe_emit(ctx: &ProgressCtx, path: &Path) {
    let n = ctx.scanned.fetch_add(1, Ordering::Relaxed) + 1;
    let now = epoch_millis();
    let last = ctx.last_emit_ms.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 200 && ctx.last_emit_ms.compare_exchange(last, now, …).is_ok() {
        let _ = ctx.window.emit("ai-cache-scan-progress", AiCacheScanProgressEvent { … });
    }
}
```

- 上限约 5 事件/秒/工具；扫描 100 万个文件也不会淹没有 WebView。
- 清理事件 `ai-cache-clean-progress` 按 target 发（通常 < 200 条，无需节流）。

### 6.9 清理实现（`clean.rs`）

```
clean_all(platform, lookup, request, sink):
  1. validate_request：targets 非空且 ≤ 64；tool 存在；category.is_cleanable()（否则 CATEGORY_NOT_CLEANABLE）；
     工具未降级（否则 TOOL_DISABLED，整批拒绝）
  2. 逐条 validate_target()：
       - 绝对路径、无 `..`/`.` 组件
       - 白名单模板精确匹配（含 dir/file 类型匹配，§7.8）
       - 降级护栏（工具级，§7.7）
       - `symlink_metadata` 判断：是符号链接（含 Windows junction）→ Skipped + warning（**先判断再 canonicalize**）
       - canonicalize 后必须在某个 canonical root 之内；target ≠ root / home / 盘符根
       - 凭据 basename 黑名单兜底
  3. 按 canonical path 去重；祖先覆盖后代（避免"先删父再删子"的重复记账）
  4. dryRun=true：每条 measure_bytes() → WouldDelete，零副作用返回
  5. dryRun=false（v1 永久删除）：
       - 删除前再次 symlink_metadata 检查（TOCTOU 兜底）
       - 记录实际字节：先 measure_bytes()，再按实际类型删除：
         目录 → fs::remove_dir_all；文件 → fs::remove_file
       - 成功 → Deleted + freed_bytes 累加；失败 → Failed + error（不重试、不降级）
       - 每删一条 emit clean 进度事件
  6. 返回汇总（deleted/skipped/failed + freedBytes + warnings）
```

### 6.10 清理请求分批约束（≤64 项/批）

- 后端硬限制：`MAX_TARGETS_PER_REQUEST = 64`；`targets.len() > 64` → 命令级 `INVALID_ARGUMENT`
  （文案：单次清理最多 64 项）。
- 前端负责分批：把「已选路径集合」按 64 个一组切片，**串行**调用 `clean_ai_caches`（避免并发删除互相干扰），
  再合并结果：
  - `freedBytes` / `deletedCount` / `skippedCount` / `failedCount` 累加；
  - `items` 按批拼接（保持选择顺序）；
  - `warnings` 去重合并；某批命令级 `Err` → 停止后续批次，保留已完成批的结果并展示错误。
- dry-run 预览同样分批，合并后一次性展示，保证"预览清单 = 实际清理清单"。
- 64 既是后端上限也是前后端协议约束：前端不得假设"一次能删完"，进度以批为单位推进。

### 6.11 扫描取消（v1 边界，文案必须诚实）

- **v1 不做后端中断**：Rust 扫描在 `spawn_blocking` 中跑到结束（`WalkDir` 无取消点），不提供
  `cancel_scan` 命令、不加中断句柄。
- 前端只做"停止显示结果"：用户点击后递增 `scanTokenRef`、丢弃后续结果与进度事件、恢复可操作状态；
  **后台扫描仍会跑完**，完成后结果被丢弃（无害，只消耗一次 CPU/IO）。
- 文案禁止暗示"已中断后台任务"；实现口径（已落地）：
  - 按钮：「停止显示结果」
  - 提示：「已停止显示结果；后端扫描仍会在后台完成，稍后可点「重新扫描」刷新。」
  - 按钮 `title`：「只停止界面显示与计时，后端扫描任务仍会跑完」
- 真取消（`AtomicBool` + `AppHandle` state，或 `WalkDir` 迭代器内检查）放 v2；届时同步改文案与 UI。
- `scanning` 期间清理按钮禁用；「停止显示结果」后允许基于上一次扫描结果继续操作。

---

## 7. 安全校验方案（分层，缺一不可）

### 7.1 信任边界

前端是**不可信输入**：即使 UI 只发自己扫出来的路径，也要假设有人用 devtools/脚本直接 `invoke`。
因此 `clean` 不信任 `path`，只把它当"候选"，最终判定以 `catalog.rs` 白名单解析结果为准。

### 7.2 校验顺序（任一失败即拒绝该条）

1. 请求自洽（命令级，整批拒绝）：targets 非空且 ≤ 64；toolId 存在；category 必须 `is_cleanable()`
   （sessions / model_cache 直接 `CATEGORY_NOT_CLEANABLE`）；工具未降级（`TOOL_DISABLED`，§7.7）。
2. 路径形态（单条）：`Path::is_absolute()`；组件中无 `ParentDir`/`CurDir`（防 `../../`）；长度非空。
3. 白名单类型匹配（单条，B1）：目标必须命中该工具该类别白名单模板；目录类模板要求目标是目录，
   文件类模板要求目标是普通文件（同名目录/文件一律不命中，§7.8）。
4. 目标类型（**先于 canonicalize**）：`symlink_metadata`（**不 follow**）；`file_type().is_symlink()` 为真
   （Windows junction 也算）→ `Skipped`。顺序很重要：先判断再 canonicalize，否则符号链接会被"解析掉"而漏检。
5. 根目录解析：取该 tool 的所有 `RootSpec`，逐个 canonicalize；`target` canonicalize 后必须在某个
   canonical root 前缀内（`ROOT_NOT_FOUND` / `PATH_NOT_ALLOWED`）。
6. 兜底禁删：target ≠ root、≠ home、≠ 盘符/文件系统根；basename 不在 §5.4 黑名单。
7. `dry_run` 与真实删除共用同一套校验，**dry-run 不是"跳过校验的预览"**。

### 7.3 canonicalize 细节与跨平台

- 用 `std::fs::canonicalize` 解析：macOS/Linux 得到真实绝对路径；Windows 得到 `\\?\C:\...` verbatim 前缀。
  **比较时必须两边都 canonical**，否则 `\\?\` 前缀会导致 `starts_with` 假阴性。
- Windows 路径大小写不敏感：比较前统一 `to_string_lossy().to_lowercase()`（或封装 `path_eq/path_under` 辅助函数）。
- 目标在扫描与清理之间被删除 → `canonicalize` 失败：在 `PATH_NOT_FOUND` 分支处理（Skipped，不当崩溃）。
- 展示路径用"解析后的逻辑路径"（不含 `\\?\`），不要回显 canonical 形式。
- 根目录本身是符号链接（`tool-data-migrate` 迁移后的典型形态）：允许作为**容器**（canonicalize 后做前缀基准），
  但删除时的 target 若本身是符号链接则跳过（§7.4）。

### 7.4 符号链接 / junction 防逃逸

- 扫描：`WalkDir.follow_links(false)`，符号链接条目跳过，不计体积、不进删除候选。
- 清理：**拒绝**删除任何最终目标是符号链接（含 Windows junction / mount point；Rust 的
  `FileType::is_symlink` 对 junction 也返回 true）。理由：目录本身可能是用户把缓存指到外部的链接，
  跟随删除会越过白名单边界。
- 若目标目录内部含符号链接 → 不 follow；`remove_dir_all` 本身不跟随内部链接（Rust 标准库保证），
  仍保留"内部 symlink 只删链接、不删链接指向"的语义。
- 残留 TOCTOU（检查与删除之间的毫秒级替换）在本地单用户桌面场景接受；删除前紧邻一次 `symlink_metadata`
  已把窗口压到最小。

### 7.5 并发与竞态

- 单窗口应用，并发来自：重复点击、React StrictMode 双 effect、扫描未完成就清理。
- 前端：所有重操作按钮在 `busy` 时 disabled；effect 用 `useRef` 防重复触发；扫描用 `scanTokenRef` 做结果
  代际校验（旧请求结果直接丢弃）；清理完成后自动重扫（而非手工减数量）。
- 清理分批：>64 项时按 §6.10 串行分批，不并发调用 `clean_ai_caches`。
- Rust：无全局状态、命令幂等——重复清理同一路径第二次返回 `PATH_NOT_FOUND`（Skipped），不产生破坏。
- 不做后端互斥锁（KISS）：代价是极端情况下两次清理交错，但每条仍然走完整校验，最坏结果是重复的 Skipped。

### 7.6 删除方式（已裁决：v1 永久删除）

- **v1 只做永久删除**：不使用回收站、不提供"移到回收站"选项、不引入新依赖（`trash` 虽在 `Cargo.toml` 中，
  但本特性不调用）。
- 安全补偿（因为不可恢复）：
  - 强制先 dry-run 预览：弹窗逐条列出「工具 / 类别 / 绝对路径 / 体积 / 合计」；
  - 高危文案："清理后无法恢复"；
  - 确认按钮红色危险样式，初始焦点在「取消」；
  - 默认勾选排除 `inUse=true`（最近 10 分钟有修改）的路径，用户可手动勾选；
  - 单条失败不中断，返回失败明细，用户可先退出被占用的工具再重试。
- 删除实现：目录 `fs::remove_dir_all`（不跟随内部符号链接）、文件 `fs::remove_file`；
  目标本身是符号链接 → 跳过（§7.4）。
- 不做"删除失败自动降级/重试"；失败就是 `Failed` 返回。

### 7.7 工具根降级：仅检测（override 指向敏感位置）

根目录来自环境变量 override（`CLAUDE_CONFIG_DIR` / `CODEX_HOME` / `GEMINI_CLI_HOME`）时，用户可能把它指到
任意位置。若解析出的 root 命中以下任一条件，整个工具降级为「仅检测」：

1. `root == home`（用户主目录本身）；
2. `root` 是 home 的祖先（如 `/Users`；含 home 本身被文件系统根覆盖的情形）；
3. `root` 是文件系统根（`/`、`C:\`）。

判定用 canonicalize 后的路径 + `path_eq` / `is_within`（Windows 大小写归一），避免 `..` / 符号链接绕过。
实现位于 `catalog.rs::tool_policy_for_roots`，环境变量读取可注入以便单测。

降级后的行为（**一个字节都不删**）：

- catalog：`AiToolCatalogDto.cleanable=false`，`warnings` 返回中文原因
  （如"根目录指向用户主目录，已降级为仅检测（不提供清理）"）；
- scan：`AiToolScanDto.cleanable=false` + warnings，正常返回路径与体积（供用户判断是否配错）；
- clean：命令级 `TOOL_DISABLED` 整批拒绝；`validate_target` 内也再拦一次（双保险）；
- UI：该工具卡显示"仅检测"徽标与降级原因，勾选/清理入口禁用；「打开目录」仍可用，方便用户修正 env 配置；
- 单测：覆盖 home / home 上级 / 文件系统根 / 正常目录四种输入。

### 7.8 目录类 vs 文件类目标（B1 修复要求）

白名单里同时存在目录类目标（`Cache/`、`sessions/` 等）与文件类目标（`logs_*.sqlite`、`session_index.jsonl`、
`history.jsonl` 等）。若只做名称匹配，会出现类型混淆（例如把 `logs_2.sqlite` 做成目录，清理时命中并递归
删除整个同名目录）。

裁决：**保留文件类目标，但文件类模板只允许匹配普通文件，不允许匹配目录**。实现要求：

1. catalog 模板显式区分类型：`AiPathKind::Dir` / `AiPathKind::File`（含 `*` 的 glob 模板一律视为 File）；
   该类型同时用于 DTO 展示与后端类型校验。
2. 展开（scan）：文件类模板在 `read_dir` 结果里只收 `is_file()`；目录类模板只收 `is_dir()`。
3. 匹配（clean）：`template_matches` 命中后必须复核 `symlink_metadata` 的 `is_file()` / `is_dir()`；
   类型不符 → `PATH_NOT_ALLOWED`（不得"按实际类型照删"）。
4. 删除：目录走 `remove_dir_all`，文件走 `remove_file`。
5. 单测（B1 专项）：同名目录存在时 `logs_*.sqlite` 模板不得命中；同名普通文件存在时目录类模板不得命中；
   传"模板命中但类型不符"的路径 → 拒绝且磁盘零变化。

---

## 8. 前端结构

### 8.1 文件清单

```
src/lib/tools.ts                                   # +1 个 TOOL_ITEMS 条目（无 windowsOnly）
src/lib/tauri.ts                                   # + AI cache 类型 + 3 个封装函数
src/lib/format.ts                                  # 新增：formatBytes（新页面用；disk-heatmap 的重复实现留作后续 DRY）
src/app/tools/ai-cache-clean/page.tsx              # 页面：状态机 + 数据编排 + 清理分批
src/app/tools/ai-cache-clean/types.ts              # 视图模型（多根目录合并为 ToolView）
src/app/tools/ai-cache-clean/utils.ts              # 类别中文名、错误文案 humanizeError
src/app/tools/ai-cache-clean/components/
  ├── tool-cache-card.tsx                          # 工具卡片（折叠/展开、类别勾选、路径列表、计算占用）
  ├── category-section.tsx                         # 单个类别行（checkbox/仅展示、风险徽标、体积、懒计算按钮）
  ├── scan-progress-bar.tsx                        # 扫描进度（当前工具/路径/已扫条目/秒数）
  ├── clean-confirm-dialog.tsx                     # dry-run 结果确认（逐路径清单、合计、永久删除危险确认）
  └── clean-result-panel.tsx                       # 结果汇总 + 失败明细
```

不建 `src/components/` 目录下的公共组件：这些组件只服务本页，放路由目录内（Next App Router 只把
`page/layout/route/...` 当路由，`components/` 不会被当成路由）。

### 8.2 注册与封装

`src/lib/tools.ts`：

```ts
{
  href: "/tools/ai-cache-clean",
  title: "AI 智能体缓存清理",
  shortLabel: "AI 缓存",
  description: "检测 Claude Code、Codex、Cursor 等 AI 工具的缓存占用，按工具/类别安全清理。",
},
```

`src/lib/tauri.ts` 新增（类型与 §6.2 一一对应，全 camelCase）：

```ts
export type AiCacheCategory = "cache" | "logs" | "sessions" | "model_cache";
export type AiRisk = "safe" | "history" | "redownload";
export type AiPathKind = "dir" | "file";
// …AiCachePathDto / AiCacheCategoryDto / AiCacheRootDto / AiToolScanDto /
//   AiCacheScanResult / AiCacheCatalog / AiCacheCleanTarget / AiCacheCleanResult / 进度事件类型

export async function getAiCacheCatalog(): Promise<AiCacheCatalog> {
  return invokeTauri<AiCacheCatalog>("get_ai_cache_catalog");
}
export async function scanAiCacheUsage(
  toolIds: string[] = [],
  includeHeavy = false,
): Promise<AiCacheScanResult> {
  return invokeTauri<AiCacheScanResult>("scan_ai_cache_usage", {
    request: { toolIds, includeHeavy },
  });
}
export async function cleanAiCaches(request: {
  targets: AiCacheCleanTarget[];
  dryRun?: boolean;              // 缺省 true
}): Promise<AiCacheCleanResult> {
  return invokeTauri<AiCacheCleanResult>("clean_ai_caches", { request });
}
```

> 类型要点：`AiCachePath.inUse` / `lastModifiedMs`、`AiCacheCategoryInfo.cleanable` / `measured`、
> `AiToolScan.cleanable` / `displayOnlyBytes`、`AiCacheScanResult.totalDisplayOnlyBytes`、
> `AiToolCatalog.cleanable` / `warnings` 与 §6.2 一一对应；`cleanable` 由后端给出，前端不自行推断。

事件监听沿用现有写法（动态 `import("@tauri-apps/api/event")`）：

```ts
const { listen } = await import("@tauri-apps/api/event");
const unlisten = await listen<AiCacheScanProgressEvent>("ai-cache-scan-progress", (e) => …);
```

### 8.3 页面状态机

```
mount
 ├─ getAiCacheCatalog()（毫秒级）→ 渲染 9 张工具卡（已检测/未检测/路径/仅检测降级）
 └─ 300ms 后自动 scanAiCacheUsage()（主扫描：只算 cache/logs；进度条 + 卡片骨架）
      ├─ 扫描中：进度条（当前工具 i/n、当前路径、已扫条目、计时）；「重新扫描」禁用
      │          「停止显示结果」只停止前端等待，后端任务仍会跑完（§6.11）
      ├─ 扫描完成：cache/logs 显示体积并可勾选；sessions/model_cache 显示路径 +「计算占用」
      │    ├─ 点单工具「计算占用」→ scanAiCacheUsage([toolId], true) → 合并回该卡（measured=true）
      │    ├─ 勾选变化 → 顶部汇总条（已选 N 项 / X GB）实时更新（inUse 路径默认不勾选）
      │    └─ 点"清理选中" → 分批（≤64）cleanAiCaches({dryRun:true}) → 确认弹窗
      │           ├─ 取消 → 返回
      │           └─ 确认 → 分批 cleanAiCaches({dryRun:false}) → 结果面板 → 800ms 后自动重扫
      └─ 扫描失败/部分警告：错误条 + 重试
```

状态：`catalog`, `scan`, `selectedPaths: Set<string>`, `scanning`, `scanTaskPending`, `heavyLoading: Set<toolId>`,
`previewLoading`, `cleaning`, `preview`, `cleanResult`, `error`, `progress`, `notice`。
**Selection 用路径集合**（不是 toolId+category 结构），避免"路径消失后勾选残留"，也让 dry-run 目标就是
"用户看到的那些路径"。分批逻辑见 §6.10（串行请求 + 结果合并）。

### 8.4 组件职责

| 组件 | 职责 | 关键 props |
|---|---|---|
| `tool-cache-card` | 工具头部（名称/厂商/检测状态/仅检测降级徽标+原因/根目录/可清理体积/展示占用）、类别列表、展开路径明细、工具级全选、「计算占用」 | `tool`, `selected: Set<string>`, `onTogglePath`, `onToggleTool`, `onMeasureHeavy`, `heavyLoading`, `busy` |
| `category-section` | 类别一行：可清理类别 checkbox；仅展示类别无勾选、显示 `measured ? 体积 : 「计算占用」`；风险徽标（safe=emerald / history=amber / redownload=rose）；路径列表（含 inUse 标记） | `category`, `selected`, `onToggle`, `onMeasureHeavy`, `heavyLoading`, `expanded`, `busy` |
| `scan-progress-bar` | spinner + aria-live 文本；当前工具名/路径截断显示；不显示假百分比 | `progress`, `elapsedMs` |
| `clean-confirm-dialog` | 只读清单（按工具→类别分组、逐条绝对路径）、合计体积、"清理后无法恢复"、逐条 inUse 提示、跳过项原因、确认/取消（无删除模式选择，v1 固定永久删除） | `preview(dryRun result)`, `toolNameById`, `inUsePaths`, `onConfirm`, `onCancel` |
| `clean-result-panel` | 汇总（释放/删除/跳过/失败）、失败项表格（路径+原因）、提示"先退出占用工具后重试" | `result`, `onRescan` |

### 8.5 视觉与交互（apply frontend-design skill）

- **设计意图**：延续项目现有"浅色玻璃卡片 + 青蓝 accent"语言，做成"清理仪表盘"——信息密度高但不拥挤。
- 主题 token：沿用 `var(--card)` / `var(--card-border)` / `var(--accent)`；新增语义色仅用 Tailwind 语义类
  （`emerald`=安全、`sky`=日志、`amber`=历史、`rose`=模型/危险），不新增 CSS 变量。
- 层级：页面标题 3xl semibold → 顶部汇总条（大数字用 `tabular-nums font-mono`）→ 工具卡片 2 列
  （`sm:grid-cols-2`）→ 类别行 → 路径明细（`font-mono text-[11px]`，可横向滚动）。
- 状态：未检测工具整卡 `opacity-60` + 灰色"未检测到"徽标，仍展示预期路径（帮助用户确认是否装到别处）。
- 动效克制：卡片 hover 轻微上浮 + 展开 150ms；清理按钮 `disabled` 态明显；危险确认用红色边框卡片，不用全屏红。
- 响应式：单列在窄窗口（<640px），widget 不横向溢出；长路径 `truncate` + `title` 提示全文。
- 可访问性：checkbox 与 label 关联；展开按钮 `aria-expanded`；进度 `aria-live="polite"`；危险操作有文字确认，
  不只靠颜色区分风险。

### 8.6 平台行为

- 全部工具跨平台，**不设 `windowsOnly`**，也不依赖 `@tauri-apps/plugin-os`：平台差异由 Rust catalog 解析，
  前端只展示 Rust 返回的路径。
- 非 Tauri 环境（`npm run dev` 浏览器）→ `invokeTauri` 抛"请使用 npm run tauri:dev 启动"；页面显示现有模式的提示条，
  不做浏览器假数据。

---

## 9. 性能与容量

| 场景 | 预算 | 措施 |
|---|---|---|
| catalog（首屏） | < 50ms | 只做路径字符串拼接 + `exists()`，不 walk |
| 主扫描（9 工具、缓存机 1-5 GB、部分 Cache 目录 10 万文件） | 1–10s | `spawn_blocking` + 工具级 rayon + 事件节流；不建树；不递归 sessions/model_cache |
| 「计算占用」二次扫描（sessions / model_cache） | 按目标体积，可能 10s+ | 单工具粒度触发（`toolIds=[id]`），只扫该工具、只算展示类别，结果合并回卡片 |
| 清理（永久删除） | 与删除目标体积相关 | 先 `measure_bytes` 再删（一次遍历 + 删除）；≤64 项/批，前端串行分批 |
| 扫描极端（Cursor Cache 百万文件） | 可能 30s+ | 进度条 + 计时；可「停止显示结果」；不阻塞 UI；真取消 v2 |
| IPC payload | 小 | 聚合到 path 级，不返回文件列表；事件最多 5/s |

---

## 10. 风险与取舍

| # | 风险 | 影响 | 应对 | 阻塞? |
|---|---|---|---|---|
| 1 | AI 工具目录结构随版本漂移（Codex/Claude Code 变化最快） | 扫不到/误标 | catalog 版本化；⚠️ 条目实测后入表；白名单机制使"未知目录"只会漏清不会被误删；v2 加只读"未识别项" | 否 |
| 2 | `~/.claude/projects` 含会话明文与 auto memory | 高（删除不可恢复） | **v1 该类仅展示不可删**（`cleanable=false`），命令级 `CATEGORY_NOT_CLEANABLE` 兜底；文案点明后果 | 否 |
| 3 | 误删凭据（`.credentials.json`、`auth.json`、`oauth_creds.json`） | 极高（需重新登录，可能丢本地配置） | 凭据永不进可清理类别；§5.4 黑名单兜底；单测断言 catalog 不含凭据文件名 | 否 |
| 4 | 工具运行中文件被占用（Windows 最明显） | 部分失败 | 单条 Failed 不中断；`inUse` 默认不勾选 + 确认弹窗提示"先退出工具"；不重试、不降级 | 否 |
| 5 | `tool-data-migrate` 迁移后根/子目录是符号链接 | 忽略或报错 | 根允许为链接（canonical 前缀基准）；目标是链接 → `Skipped` + 提示"请到实际位置清理" | 否 |
| 6 | 扫描无后端中断、耗时可能 30s+ | UX / 误解 | v1 只做「停止显示结果」（前端停止等待，后台跑完）+ 诚实文案（§6.11）；真取消 v2 | 否（v2） |
| 7 | TOCTOU（检查与删除之间路径被换） | 低概率越界删除 | 删除前紧邻二次 `symlink_metadata`；本地单用户场景接受残余风险 | 否 |
| 8 | 永久删除不可恢复（无回收站） | 误删无法找回 | 强制 dry-run 预览 + 逐路径清单 + "清理后无法恢复" + 红色危险确认 + 初始焦点在「取消」；`inUse` 默认排除（§7.6） | 否 |
| 9 | macOS 权限（TCC / 完全磁盘访问）/ Linux 权限不足 | 体积统计不全 | 读失败只 warning 不中断；UI 汇总 warning 并提示授权 | 否 |
| 10 | 多编辑器根（Code / Insiders / VSCodium）或软链导致同一目录重复 | 重复计数/重复删除 | 词法 + canonicalize 去重；清理按 canonical path 再去重、祖先覆盖后代 | 否 |
| 11 | 前端 StrictMode 双 effect 触发两次扫描 | 双倍 CPU/进度跳变 | `useRef` guard + `scanTokenRef` 代际校验；Rust 命令无副作用、可重入 | 否 |
| 12 | Ollama 模型误操作（数 GB 重下载） | 用户代价大 | **v1 `model_cache` 仅展示不可删**；删除入口不存在 | 否 |
| 13 | 文件/目录类型混淆（B1）：同名目录被文件模板命中并递归删除 | 极高（越界删目录） | 白名单模板类型化（Dir/File）；文件模板只匹配 `is_file()`，目录模板只匹配 `is_dir()`；类型不符拒绝 + B1 专项单测（§7.8） | **是**（修复前阻断上线） |
| 14 | 单批目标过多（>64）导致长事务/超时 | 部分失败/无响应 | 后端 `MAX_TARGETS_PER_REQUEST=64` 硬限制；前端串行分批并合并结果（§6.10） | 否 |
| 15 | Claude Desktop 配置 / 扩展 / 状态被误删（D5） | 高（配置与扩展丢失、需重装） | 它们属于 C 类（仅展示/受保护），不进入可清理类别；Windsurf/Cursor 的扩展目录同理（不在白名单） | 否 |

**取舍说明（为什么这样做）**

- **只做白名单删除，不做"目录里除了 X 其余全删"**：少一点清理收益，换掉整类误删风险；符合 constitution 的
  "security-first"。系统缓存根（D4）已在 v1.1 backlog 中保留同样的"精确条目"原则（§16），v1 不做。
- **只清 cache / logs，sessions / model_cache 仅展示**：项目定位是"释放磁盘空间"，不是"历史管理"；
  把不可逆的会话/模型删除移出 v1，用"零删除入口"换掉最有价值的误删事故。
- **v1 永久删除、不做回收站**：回收站不释放空间、还得二次清空，与工具目标矛盾；用 dry-run + 逐路径确认 +
  `inUse` 默认排除来补偿不可恢复风险（已裁决，§12）。
- **不建树/不做可视化**：`disk-heatmap` 已有；本工具的价值是"安全的一键清理"，返回聚合体积即可，避免重演
  "大目录渲染卡死"的历史问题（`disk-heatmap` 页面已为此做过两轮性能修复）。
- **不引入 `dirs`/`thiserror`/`serde` 之外的任何依赖**：路径基址解析用 `env` 现成实现，几十行代码可控。
- **命令级 `Err(String)` 而非类型化错误**：项目现有约定如此；`CODE:` 前缀给前端留了结构化空间，
  真要类型化等第二个消费方出现再做（KISS）。

---

## 11. Rust 单元测试点（`ai_cache/tests.rs` 为本方案验收硬指标）

测试用 `tempfile`？——**项目无此依赖，不新增**。用 `std::env::temp_dir()` + 唯一子目录
（`format!("devtoolkit-ai-cache-test-{}", uuid::Uuid::new_v4())`，`uuid` 已是依赖），
`struct TempDir` 手写 `Drop` 递归删除。测试统一串行（不依赖全局 HOME）。

### 11.1 catalog / 静态安全断言

1. `catalog_tool_ids_unique`：所有工具 `id` 唯一、非空。
2. `catalog_categories_valid`：每个 `CategorySpec.category` 是 4 类枚举之一；`paths` 非空；
   无绝对路径、无 `..`、无空串；`is_cleanable()` 只对 cache / logs 返回 true。
3. `catalog_never_lists_protected_basenames`：所有可清理路径的 basename（含 glob 去 `*`）不在 §5.4 黑名单里
   —— 这是"凭据永不进白名单"的机器化保证。
4. `catalog_category_paths_do_not_nest_protected`：可清理路径不与同工具的 `protected` 列表互为祖先/后代。
5. `resolve_roots_expands_bases`（`cfg` 按当前平台）：当前平台每个工具的 roots 非空、全部绝对路径、
   无重复（canonical 去重后）。
6. `env_override_wins`：把 override 值作为参数注入 `resolve_roots_with(spec, platform, env_lookup_fn)`
   （生产用闭包读 `env::var_os`，单测用假查找器；**设计要求 env 读取可注入**），断言返回 override 目录。
6b. `file_templates_match_only_files`（B1）：`logs_*.sqlite` 模板在"同名目录 + 普通文件"混合目录中
   只收文件；`expand_targets` / `template_matches` 对目录命中返回 false。

### 11.2 路径校验（拒绝用例必须有）

7. `validate_rejects_empty_and_relative`。
8. `validate_rejects_parent_traversal`：`<root>/../secret` 被拒。
9. `validate_rejects_outside_root`：把 `/tmp/other/...` 传成 claude-code 目标 → `PATH_NOT_ALLOWED`。
10. `validate_rejects_root_itself`：传 root 本身 → 拒。
11. `validate_rejects_non_allowlisted_subdir`：`<root>/settings.json`（受保护）→ 拒。
12. `validate_rejects_symlink_target`：`<root>/paste-cache` 换成指向外部目录的符号链接 → `PATH_IS_SYMLINK`（Unix 用 `std::os::unix::fs::symlink`；Windows 用 `std::os::windows::fs::symlink_dir`，`cfg` 分开或 `#[cfg(unix)]` 标注）。
13. `validate_rejects_file_bypass`：把 `history.jsonl` 标成 `cache` 类别 → 拒（类别不符白名单）。

### 11.3 扫描

14. `scan_missing_root_is_not_error`：临时 HOME/根不存在 → `detected=false`、字节 0、无 Err。
15. `scan_sums_only_allowlisted_dirs`：伪造根目录：`paste-cache/a.bin`(100B)、`settings.json`(999B)、
    `random/x.bin`(50B) → cache 类别 100B，settings/random 不计。
16. `scan_skips_symlink_children`：cache 目录内放符号链接指向外部大文件 → 体积不计、无 panic。
17. `scan_real_files_and_glob`：`logs_1.sqlite`(10B) + `logs_2.sqlite`(20B) 命中 `logs_*.sqlite` → 30B；
    `logs_3.txt` 不命中。
18. `scan_category_aggregation`：同一 tool 多 root（伪造两个 VS Code 根）→ 各自 `exists` 正确、总字节为和。
19. `scan_result_serializes_camel_case`：`serde_json::to_value` 断言含 `cleanableBytes`/`displayOnlyBytes`/
    `totalDisplayOnlyBytes`/`catalogVersion`，category 值为 `"model_cache"`（契约测试，仿 `lib.rs` 现有 serde 测试）。
19b. `scan_main_scan_skips_heavy_categories`：`includeHeavy=false` 时 sessions / model_cache 的
    `measured=false`、`bytes=0`、`displayOnlyBytes=0`；`includeHeavy=true` 时 `measured=true` 且体积正确。
19c. `in_use_flag_from_mtime`：新写入的 target `inUse=true`、`lastModifiedMs` 接近当前时间；
    `touch` 到旧时间的 target `inUse=false`（阈值为 10 分钟）。

### 11.4 清理

20. `clean_dry_run_touches_nothing`：`dryRun=true` 后文件仍存在；items 全 `would_delete`；`freedBytes` 正确。
21. `clean_permanent_removes_only_selected`：只删 `paste-cache`，`settings.json` 与 `random/` 仍在。
22. `clean_rejects_display_only_categories`：targets 含 sessions / model_cache → 命令级 `Err(CATEGORY_NOT_CLEANABLE)`，
    磁盘零变化。
23. `clean_skips_symlinked_target`：目标是符号链接 → `Skipped/PATH_IS_SYMLINK`，外部目录未受影响。
24. `clean_rejects_unknown_tool_and_category`：`TOOL_NOT_FOUND` / `CATEGORY_NOT_CLEANABLE`。
25. `clean_rejects_over_64_targets`：65 项 → 命令级 `Err(INVALID_ARGUMENT)`，磁盘零变化。
26. `clean_dedupes_nested_targets`：同时传 cache 父目录与其子路径 → 只删父、只记一次。
27. `clean_reports_partial_failure`：一个目标不存在 → 该项 `Skipped`，其它项 `Deleted`，整体 `Ok`。
28. `clean_result_serializes_camel_case`：`freedBytes`/`deletedCount`/`dryRun`（序列化后**无** `deletionMode`）。
28b. `clean_second_run_is_idempotent`：连续两次清理同一目标 → 第二次 freed=0、deleted=0、skipped=1
    （`PATH_NOT_FOUND`），不报失败。
28c. `clean_bc_files_untouched`：清理 cache/logs 后 B/C 类文件（配置、凭据、未知目录）数量与 SHA-256 不变。

### 11.5 手工验证矩阵（Tester）

| 平台 | 用例 | 期望 |
|---|---|---|
| macOS | 本机装 Claude Code / Cursor 等 | 自动检测、体积与 `du -sh` 同数量级（±10%）、不卡 UI |
| macOS | Claude Desktop 配置与扩展（D5） | `claude_desktop_config.json`、`extensions/`、`config.json`/`state*` 不出现在可清理列表，磁盘不变 |
| macOS | 用 `tool-data-migrate` 把 `~/.claude` 软链到外盘 | 正常检测到实际路径；删除时不跟随/不越界；给出提示 |
| macOS | sessions / model_cache 类别 | 只有「计算占用」与「打开目录」，**没有任何删除入口** |
| Windows | 工具运行中执行清理 | 部分 Failed，提示"先退出工具"；无崩溃 |
| Windows | 路径大小写不同的伪造 target 调用 | 正常命中白名单（大小写不敏感） |
| 三平台 | `cleanAiCaches` 一次传 65 项 | 命令级拒绝（"单次清理最多 64 项"），磁盘零变化 |
| 三平台 | 把 `CLAUDE_CONFIG_DIR` 指向 `$HOME` 后打开页面 | 工具卡显示"仅检测"+原因；勾选/清理禁用；扫描正常 |
| 三平台 | 扫描中点「停止显示结果」 | 文案说明后台仍会完成；UI 恢复可操作；完成后结果不覆盖（需重新扫描） |
| 三平台 | 手工 `invoke` 传 `path: "/"` / `"../../x"` / 受保护文件 / 文件模板命中目录 | 全部被拒，无任何删除 |
| 三平台 | `npm run lint`、`cargo fmt --check`、`cargo clippy`、`cargo test` | 全绿 |

### 11.6 回写新增用例（工具根降级 / 类型匹配 / 分批 / D5）

29. `tool_policy_degrades_on_home_root`：override 指向 `$HOME` → `cleanable=false` + 中文 warning。
30. `tool_policy_degrades_on_home_ancestor`：override 指向 home 的上级 → 降级。
31. `tool_policy_degrades_on_filesystem_root`：override 指向 `/`（Windows 用 `C:\`）→ 降级。
32. `tool_policy_allows_normal_root`：override 指向临时目录下的 `.claude` → `cleanable=true`、warnings 为空。
33. `degraded_tool_cannot_be_cleaned`：降级工具的 clean 请求 → 命令级 `TOOL_DISABLED`，磁盘零变化。
34. `catalog_exposes_cleanable_and_warnings`：catalog 与 scan 的 `cleanable` / `warnings` 字段出现在 JSON 中且语义一致。
35. `file_template_type_mismatch_rejected`（B1）：把同名目录路径按文件模板提交 → `PATH_NOT_ALLOWED`，目录未被删除。
36. `claude_desktop_config_extensions_not_cleanable`（D5）：`claude_desktop_config.json`、`extensions`、
    `config.json`、`state*` 不在任何可清理类别；scan 不返回为 target；clean 传入被拒。
37. `tool_matrix_stays_nine`（D5）：`tools().len() == 9`；v1 不新增工具条目（系统缓存根设计属 backlog）。

> 系统缓存根（D4）相关测试点 36–39（旧编号）已移出 v1 验收，见 §16.5。

---

## 12. 裁决记录（Chief，已裁决）

| # | 事项 | 裁决（已裁决） | 落地方式 |
|---|---|---|---|
| D1 | 删除方式 | **v1 只做永久删除**：不提供回收站选项、不引入 `trash` 相关依赖（零新增依赖） | `clean.rs` 用 `fs::remove_dir_all` / `fs::remove_file`；DTO 无 `deletionMode` 字段（§6.2 / §7.6） |
| D2 | sessions 类别 | **v1 仅展示，不提供清理** | `AiCacheCategory::is_cleanable()` 只对 cache/logs 为 true；`cleanable=false`；clean 命令级 `CATEGORY_NOT_CLEANABLE` |
| D3 | Ollama model_cache | **v1 仅展示，不提供清理** | 同上；模型体积照常展示，无删除入口 |
| D4 | 系统缓存根（macOS 等） | **最终裁决：不进 v1，留 backlog（v1.1）**。代码中无 `claude-cli-nodejs` / `~/Library/Caches` 条目，v1 不扫描、不清理系统缓存根；已评审的精确条目设计移入 §16（设计已备，未实现） | v1：无相关代码/命令/测试；v1.1：§16（`RootKind::SystemCache` + 根级 `overrides` + `children_globs`，精确条目、禁止整根/通配） |
| D5 | 工具矩阵与 Claude Desktop 边界 | **保持 spec v1.1 的 9 工具矩阵不变**；Claude Desktop 的 `claude_desktop_config.json`、扩展安装目录、`config.json` / `state*` 等配置与状态文件归「仅展示 / 受保护」，不进入可清理类别 | §5.2/§5.3.2；`tools().len()==9` 单测；C 类 protected |
| D6 | 白名单文件类目标（Reviewer B1） | **保留文件类目标，但只允许匹配文件，不允许匹配目录** | 模板类型化 Dir/File；`expand_targets` / `template_matches` 复核 `is_file()` / `is_dir()`；类型不符 `PATH_NOT_ALLOWED`；专项单测（§7.8） |
| D7 | 工具根降级 | 由 env override 得到的 root == home / home 的祖先 / 文件系统根 → 该工具降级为「仅检测」（`cleanable=false`），catalog 与 scan 返回 warnings | §7.7；`tool_policy_for_roots`；clean 命令级 `TOOL_DISABLED`，单条校验再拦一次 |
| D8 | 清理请求分批 | **单次最多 64 项**，前端负责分批调用并合并结果 | §6.10；后端 `MAX_TARGETS_PER_REQUEST`，超出命令级 `INVALID_ARGUMENT` |
| D9 | 扫描取消 | **v1 不做后端中断，只做前端停止等待**，文案必须诚实；真取消放 v2 | §6.11；按钮「停止显示结果」+ 后台仍会完成的说明 |

已落定、无需再拍板：首屏自动扫描（挂载后 300ms 触发）、「全选安全缓存」按钮、新增 Rust 模块目录、
全平台不设 `windowsOnly`、不新开服务端/数据库、不改 Tauri 配置与 capabilities。

**回写说明**：本版把初稿中的 `deletionMode` / `riskAck` / 默认回收站 / sessions+model_cache 可清理
等设计全部改为实现口径；DTO 增补 `cleanable` / `inUse` / `lastModifiedMs` / `displayOnlyBytes` /
`measured` / `totalDisplayOnlyBytes`；**D4 按最终裁决移出 v1**（系统缓存根设计移入 §16 backlog），
D5（9 工具矩阵不变 + Claude Desktop 配置/扩展/状态仅展示）保留。QA 验收以本版为准
（旧版 plan 的上述字段与行为已作废）。

---

## 13. 实施任务拆分（交给 Coder，按序）

> 每步结束都应能通过 `cargo fmt --check` + `cargo clippy` + `cargo test`（T1 起），最后 `npm run lint` + `npm run build`。
> 单文件写作用：Rust 侧全部为新增文件 + `lib.rs` 两处小改；前端全部为新增文件 + `tools.ts`/`tauri.ts` 追加。

| 任务 | 内容 | 验收 |
|---|---|---|
| T1 | 建 `ai_cache/` 模块骨架；`catalog.rs` 写入 9 个工具常量（⚠️ 条目注释掉）+ 基址/环境变量解析（env 可注入）；**不含任何系统缓存根**（D4 留 v1.1）；`tests.rs` 写 11.1 与 11.6 的 36–37 | `cargo test` 通过；`id` 唯一且共 9 个；claude-code 条目与 §6.6 一致 |
| T2 | `clean.rs` 的 `validate_target` + 白名单匹配（含 dir/file 类型匹配，B1）+ 符号链接/root/home 拒绝 + 工具根降级护栏；11.2 + §11.6 的 29–37 测试 | 拒绝用例全绿；类型不符/降级工具均被拒；无 `unwrap()` on user input |
| T3 | `scan.rs`：`measure_bytes` + 并行扫描 + 进度节流 + `includeHeavy` 惰性类别 + `inUse` / `lastModifiedMs`；11.3 测试 | 伪造目录体积精确；主扫描不递归 sessions/model_cache；符号链接不计数 |
| T4 | `clean_all`：dry-run、永久删除、≤64 项限制、去重、部分失败；三个 command + `lib.rs` 注册；11.4 测试 | dry-run 零副作用；permanent 只删选中；65 项被拒；部分失败整体 Ok |
| T5 | `tauri.ts` 类型 + 3 个 wrapper；`tools.ts` 条目；`lib/format.ts` | `tsc`/lint 通过；类型与 Rust DTO 字段一一对应 |
| T6 | 页面骨架：catalog 首屏 + 自动扫描 + 进度条 +「停止显示结果」+ 错误态 | 打开即见 9 卡片与进度；停止显示文案诚实（后台仍完成） |
| T7 | 卡片/类别/勾选/汇总条 +「计算占用」+ dry-run 确认弹窗 + 结果面板 + ≤64 分批 + 清理后重扫 | 手动走通"扫描→（计算占用）→勾选→预览→取消→确认→重扫"；>64 项自动分批 |
| T8 | 三平台手工矩阵（§11.5）+ 安全负例（手工 invoke 非法 path） | 安全负例全部被拒；UI 无冻结 |

**提交**：`feat: 新增 AI 智能体缓存清理工具页`（Conventional Commits + 中文），PR 目标 `main`；
按 constitution 走 architect 设计评审（本文件）→ senior-qa 测试评审 → staff-infrastructure 构建/跨平台评审。

---

## 14. 附录：目录/接口速查

- Rust 入口：`src-tauri/src/lib.rs:2276`（`run()` 的 `generate_handler!`）。
- 扫描范式参考：`src-tauri/src/lib.rs:909-957`（`scan_disk_usage`）。
- 进度事件范式：`src-tauri/src/lib.rs:731-737`（`emit_disk_scan_progress`）+ 前端 `disk-heatmap/page.tsx:562`。
- 删除参考（**不复用**）：`src-tauri/src/lib.rs:1061-1076` 的 `delete_path` 走回收站；本特性按裁决只用
  `fs` 永久删除，不调用 `trash`。
- 路径校验范式（Windows 迁移）：`src-tauri/src/lib.rs:1148-1191`。
- 前端 IPC 封装：`src/lib/tauri.ts:9-16`。
- 工具注册表：`src/lib/tools.ts:31-76`。
- 页面外壳：`src/app/tools/layout.tsx`（导航 + 最近使用）。

---

## 15. 参考来源（路径事实核验）

- Claude Code 应用数据目录（transcripts/caches/logs 分类、`CLAUDE_CONFIG_DIR`）：
  <https://code.claude.com/docs/en/claude-directory>
- Cursor 各平台数据根与聊天历史库位置（`User/globalStorage/state.vscdb`）：
  <https://github.com/getagentseal/codeburn/blob/main/docs/providers/cursor.md>
- Gemini CLI 目录布局（`~/.gemini/tmp/<slug>/chats`、`shell_history`、`oauth_creds.json`、`GEMINI_CLI_HOME`）：
  <https://r3nzsec.github.io/irflow-timeline/dfir-tips/ai-apps/gemini>
- Codex `~/.codex` 目录结构（sessions / logs_*.sqlite / plugins/cache / auth.json 等）：
  <https://cloud.tencent.com.cn/developer/article/2690085>
- Electron 系（Claude Desktop / Windsurf）Chromium 缓存目录命名沿用标准 userData 布局，v1 标 ✅，
  实现时以真实安装 `ls` 复核。

---

## 16. v1.1 backlog：系统缓存根清理（设计已备，未实现）

> **状态：不进 v1（D4 最终裁决；对应 spec v1.3 §12「v1.1 backlog」）。**
> `src-tauri/src/ai_cache/` 中没有任何 `claude-cli-nodejs` /
> `~/Library/Caches` / `~/Library/Logs` 相关条目；v1 不扫描、不清理系统缓存根。本节保存已评审的设计，
> 实施前需先改 spec（把 D4 / R15 标到 v1.1）并补测试，再单独开 PR。

### 16.1 目标

在 9 个工具矩阵不变的前提下，为已有工具挂**系统缓存根**（macOS `~/Library/Caches/<app>`、
`~/Library/Logs/Claude`，Linux `~/.cache/claude-cli-nodejs`），按**精确条目**清理 MCP 日志与
Electron 系统缓存。收益：Claude Code MCP 日志在异常场景可达数十 GB 级增长。

### 16.2 catalog 设计（v1.1 拟新增）

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    ToolData,      // 默认：~/.claude、~/Library/Application Support/Cursor
    SystemCache,   // 系统缓存根：只允许精确条目，禁止整根/整层通配
}

pub struct RootSpec {
    pub platforms: &'static [Platform],
    pub base: BaseDir,                      // 新增 BaseDir::MacLibraryCaches / MacLibraryLogs
    pub relative: &'static str,
    pub env_override: Option<&'static str>,
    pub kind: RootKind,
    /// 空 = 继承工具级 categories；系统缓存根必须显式覆盖。
    pub overrides: &'static [CategorySpec],
    /// 仅日志根（~/Library/Logs/Claude）：允许根目录本身作为唯一目标。
    pub root_as_target: bool,
}

pub struct CategorySpec {
    pub category: AiCacheCategory,
    pub paths: &'static [&'static str],
    /// 系统缓存根专用：「根下每个直接子目录」内再匹配末段模式（如 "--mcp-logs-*"）。
    /// 父目录名由 read_dir 实测枚举，配置里不允许写 `*` / `*/*` 段。
    pub children_globs: &'static [&'static str],
}
```

### 16.3 系统缓存根与条目

| 工具 | 平台 | 系统缓存根 | 精确白名单条目（相对根） |
|---|---|---|---|
| `claude-code` | macOS | `~/Library/Caches/claude-cli-nodejs` | `<项目目录>/--mcp-logs-*` |
| `claude-code` | macOS | `~/Library/Logs/Claude` | 根目录本身（日志根，`root_as_target=true`） |
| `claude-code` | Linux | `~/.cache/claude-cli-nodejs` | `<项目目录>/--mcp-logs-*` |
| `cursor` | macOS | `~/Library/Caches/Cursor` | `Cache`、`Code Cache`、`CachedData`、`GPUCache`、`DawnGraphiteCache`、`DawnWebGPUCache`、`GrShaderCache`、`ShaderCache` |
| `windsurf` | macOS | `~/Library/Caches/com.exafunction.windsurf` | 同 Cursor 的系统缓存根条目 |
| 任一工具 | Windows | — | 结构未实测，不启用（不猜路径） |

`<项目目录>` 必须来自该根下的实际磁盘枚举（单层目录名，不含分隔符 / `..`）；只删名称以 `--mcp-logs-`
开头的子目录本身——不删项目目录、不删缓存根、不做 `*/*` 通配。

### 16.4 校验规则（原 §7.9）

1. 白名单只能来自 `RootSpec.overrides` 的 `paths` / `children_globs`；catalog 期断言配置里不出现
   `*` 作为独立段（`children_globs` 的父目录由 `read_dir` 实测枚举）。
2. `children_globs` 展开：枚举根下每个直接子目录（跳过 symlink），在子目录内按末段模式匹配；
   父目录名必须是单层目录名；匹配到的目标仍要过 v1 的全套校验（§7.2）。
3. **不删系统缓存根本身**：仅 `root_as_target=true` 的日志根允许目标 == 根；其余一律 `ROOT_PROTECTED`。
4. **不整层通配**：没有"删根下所有子项"的分支；根下无匹配条目 → 0 项，UI 显示"无可清理项"，
   不退回整根删除。
5. 降级（§7.7）仍只针对工具数据根；系统缓存根解析异常到 home / 盘符根时拒绝并 warning。

### 16.5 测试点（迁出 v1 验收）

- `system_cache_expands_children_globs`：伪造 `~/Library/Caches/claude-cli-nodejs/<proj>/--mcp-logs-x/`、
  `<proj>/other/`、`<proj>/` → 只返回 `--mcp-logs-x`；`<proj>` 与 `other` 不返回。
- `system_cache_never_deletes_root_or_parent`：clean 传缓存根本身或 `<proj>` → `ROOT_PROTECTED` /
  `PATH_NOT_ALLOWED`，磁盘零变化。
- `system_cache_no_match_is_empty`：根存在但无匹配条目 → 0 个 target，不退回整根删除。
- `system_cache_exact_subdirs_only`（Cursor）：伪造白名单子目录 + 未列出子目录 → 只返回白名单子目录。

### 16.6 实施前置

- spec 改版（D4 / R15 标 v1.1）与 AC 更新；
- `catalog.rs` 增加 `RootKind` / `overrides` / `children_globs` 与系统缓存根条目；
- 上述 4 个单测 + 手工矩阵；
- 独立 PR，按 architect → senior-qa 流程评审。
