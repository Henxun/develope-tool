---
feature: ai-cache-clean
status: draft
owner: 产品（Product）
created: 2026-10-31
version: 1.0
target: v1（MVP）
---

# AI 智能体缓存清理 · 功能规格（v1）

> 一句话：自动检测本机已安装的 AI 编码工具，列出**可安全清理的缓存/日志目录**及其占用，让用户一键释放磁盘空间，同时明确展示"会话记录、配置、凭据"等**永不删除**的数据。

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
2. 清理动作**只作用于白名单目录**；清理前后会话记录/配置/凭据的字节级内容零变化（AC-13）。
3. 一次清理的成功率 ≥95%（目录不存在/符号链接/占用 = 正常跳过，不算失败）。

### 1.4 非目标

- 不是通用磁盘清理器（不碰 Xcode、npm、Docker、系统缓存）。
- 不管理模型权重（Ollama / LM Studio / HuggingFace 见 §9）。
- 不做"迁移/搬家"，那是已有的「工具数据迁移」。

---

## 2. 范围与工具覆盖

### 2.1 v1 工具矩阵

| 工具 | 检测根目录 | 平台 | v1 能力 |
|------|-----------|------|---------|
| Claude Code | `~/.claude`（可用 `CLAUDE_CONFIG_DIR` 覆盖）；macOS 另加 `~/Library/Caches/claude-cli-nodejs/` | macOS / Windows / Linux | 清理 A 类 + 只读展示 B 类 |
| Codex CLI | `~/.codex`（可用 `CODEX_HOME` 覆盖） | 三平台 | 清理 A 类 + 只读展示 B 类 |
| Cursor | macOS `~/Library/Application Support/Cursor` + `~/Library/Caches/Cursor`；Windows `%APPDATA%\Cursor`；Linux `~/.config/Cursor` | 三平台 | 清理 A 类 + 只读展示 B 类 |
| Windsurf | macOS `~/Library/Application Support/Windsurf` + `~/Library/Caches/com.exafunction.windsurf`；Windows `%APPDATA%\Windsurf`；Linux `~/.config/Windsurf` | 三平台 | 清理 A 类 + 只读展示 B 类 |
| GitHub Copilot CLI | `~/.copilot` | 三平台 | 清理 A 类（`logs/`，存在才显示）+ 只读展示 |
| Gemini CLI | `~/.gemini` | 三平台 | **v1 只检测、只展示、不清理**（原因见 §2.2 与 §10-D1） |

> 判定"已安装"的唯一标准：**检测根目录存在**。不存在 → 该工具不出现在列表中。
> 根目录存在但 A 类目录全都不存在 → 仍显示该工具卡片，标注「已检测到 · 暂无可清理缓存」，并展示 B 类占用。

### 2.2 数据分级（本规格的核心模型）

| 级别 | 定义 | 产品行为 |
|------|------|----------|
| **A 类：可清理缓存** | 删除后**无用户可见内容损失**，工具下次使用会按需重建（缓存、日志、调试残留、旧版本遗留目录、安装包副本） | 可勾选、可清理，默认勾选 |
| **B 类：用户数据** | 会话记录、编辑历史、插件、技能、记忆、配置、模型缓存等，删除会造成**不可逆的信息损失** | 只在界面展示（名称 + 占用），提供「打开目录」，**无勾选框** |
| **C 类：绝不触碰** | 凭据、账号、配置、根目录本身、符号链接目标、白名单外的一切 | 不展示为可清理项；后端拒绝任何删除请求 |

**一句话规则：白名单之外的一切都不删；文件永不删除（v1 只删目录）。**

### 2.3 检测规则

1. 根目录 = 环境变量优先（`CLAUDE_CONFIG_DIR`、`CODEX_HOME`）→ 否则默认路径；不允许用户在界面自定义路径。
2. 平台路径必须由 `$HOME` / `%USERPROFILE%` / `%APPDATA%` 等系统变量推导，**禁止硬编码用户名**。
3. 不读注册表、不调用外部 CLI、不联网。
4. 工具清单必须是**数据驱动**的（一张表 + 一张白名单表），新增工具只需加配置与测试，不改界面逻辑。

---

## 3. MVP 功能清单

### 3.1 v1 必须（P0，缺一不可上线）

| ID | 功能 | 说明 |
|----|------|------|
| F1 | 工具入口 | 路由 `/tools/ai-cache-clean`，注册进工具导航（所有平台可见，非 Windows 专属） |
| F2 | 自动检测 | 进入页面即扫描全部 6 个工具根，存在的才展示 |
| F3 | 占用统计 | 展示每个工具 A 类各项字节数、各工具小计、全局"可清理总量" |
| F4 | 分级展示 | A 类可勾选（默认全选，含一句话说明）；B 类只读展示 + 「打开目录」 |
| F5 | 按需计算 B 类体积 | B 类默认只显示名称，点击「计算占用」后再递归统计（避免拖慢主扫描） |
| F6 | 确认后清理 | 勾选 → 点击「清理选中项」→ 确认弹窗逐条列路径与合计 → 确认后执行 |
| F7 | 结果反馈 | 展示成功/失败明细、实际释放量；自动重扫刷新数字 |
| F8 | 安全校验 | 后端白名单 + 路径规范化 + 符号链接/根目录/越界拒绝（见 §4.1、§6） |
| F9 | 异常隔离 | 单项失败不影响其他项；无权限/占用/不存在都给中性提示 |
| F10 | 运行中保护 | 目录内 10 分钟内有修改 → 标「可能正在使用」，默认不勾选 |
| F11 | 空态/错误态 | 无工具、全部无缓存、读取失败 均有明确文案 |
| F12 | 测试 | 白名单拒绝、符号链接拒绝、B 类零影响、单项失败隔离 四类 `cargo test` |

### 3.2 后续版本（明确不在 v1，别顺手做）

| 版本 | 内容 | 触发条件 |
|------|------|----------|
| v1.1 | Cursor / Windsurf `workspaceStorage`（`.pack` 编辑历史）按工作区 + 30 天未使用筛选清理 | 见 §10-D1 的拍板结果 |
| v1.1 | Gemini CLI `tmp/` 中按时间的会话快照清理 | 明确"会话记录"的保留策略后 |
| v1.2 | 「移至回收站」模式（可恢复，但需手动清空才释放空间） | 用户提出"要能撤销" |
| v1.2 | 定时扫描提醒 / 阈值告警（>N GB 提示） | 不做后台常驻，仅打开页面时提示 |
| v2 | 新增工具：Cline / Roo Code / Continue / Zed / Trae、模型缓存（Ollama、HuggingFace） | 各自路径先固化进白名单 + 测试 |

### 3.3 不做什么（v1 硬性排除）

1. **不做任意路径清理**：不提供"自定义目录""手动输入路径""全盘搜索大文件"。
2. **不做通配符/正则匹配删除**（如 `~/.claude/*.log`）。
3. **不做文件级删除**：粒度只到白名单目录。
4. **不删整个工具根目录**（`~/.claude`、`~/.codex`、`~/.cursor` 永不整体删除）。
5. **不清理会话记录、配置、凭据、插件、扩展、记忆**（见 §4.3）。
6. **不动 Claude Desktop / ChatGPT Desktop / 其它 GUI 应用的缓存**（非编码工具，风险与收益不匹配）。
7. **不做回收站/系统清理**（"清空回收站"是另一类高风险功能）。
8. **不做自动清理、开机自启、后台常驻**。
9. **不联网、不上传路径或体积数据、不设遥测**。
10. **不要求管理员权限**，不触发 UAC/提权。
11. **不做删除撤销**（v1 直接删除；恢复靠工具重建缓存，文案必须写清楚）。

---

## 4. 目录白名单与绝不触碰清单（写死）

### 4.1 全局硬规则（后端必须逐条实现）

| 规则 | 内容 |
|------|------|
| R1 | 白名单**硬编码在后端**（Rust 常量表）。前端只能传 `toolId + cacheId`，**永远不能传路径**。 |
| R2 | 实际路径 = 规范化(工具根) + 白名单相对路径；规范化后必须仍在工具根之下；含 `..`、绝对路径、空 id、未知 id → 拒绝并返回错误。 |
| R3 | 只删白名单中列出的**目录**；不做前缀匹配、不做通配、不删文件。 |
| R4 | 白名单目录若是**符号链接 / junction / reparse point** → 跳过（不跟随、不删目标），结果标注"已跳过：符号链接"。 |
| R5 | 绝不删除：工具根目录本身、磁盘根、用户主目录、系统目录（`/etc`、`/usr`、`C:\Windows`、`%ProgramFiles%`）。 |
| R6 | 目录不存在 → 静默跳过，不报错、不计入失败。 |
| R7 | 目录内最近 **10 分钟**内有文件修改 → 标记「可能正在使用」，该项默认不勾选（可手动勾选）。 |
| R8 | 逐项独立执行；单项失败（权限/占用/文件被删）只影响该项，其余继续；结果中分别列出成功与失败原因。 |
| R9 | 工具根必须位于当前用户主目录内（Windows `%USERPROFILE%`、macOS `$HOME`/`~/Library`）；否则该工具整体标记"路径异常"并禁用清理。 |
| R10 | 单次清理请求最多 64 项；超出拒绝。重复清理同一项为幂等（第二次成功、释放 0 B）。 |
| R11 | 不写任何持久化记录（不落盘历史/日志/数据库）；清理结果仅在本页面内存中展示，刷新即失。 |
| R12 | 全程无网络请求；不使用 shell/PowerShell 执行删除，统一用 Rust 原生 `fs` 接口。 |

### 4.2 各工具白名单

#### 4.2.1 Claude Code

根：`$CLAUDE_CONFIG_DIR`（若设置）否则 `~/.claude`

| cacheId | 相对路径 | 内容 | 删除后影响 | 默认勾选 |
|---------|----------|------|-----------|---------|
| `claude.debug` | `debug/` | `--debug` / `/debug` 时的会话调试日志 | 无用户可见损失 | 是 |
| `claude.shell-snapshots` | `shell-snapshots/` | 启动时抓取的 shell 别名/函数快照（正常退出即清，崩溃残留） | 无 | 是 |
| `claude.session-env` | `session-env/` | 每会话的环境元数据 | 无 | 是 |
| `claude.plans` | `plans/` | plan mode 写下的计划文件 | 历史计划不可回看，无功能影响 | 是 |
| `claude.backups` | `backups/` | `~/.claude.json` 旧版本副本（保留最近 5 份） | 失去旧配置回溯副本 | 是 |
| `claude.legacy-todos` | `todos/` | 旧版本遗留任务目录（新版本不再写入） | 无 | 是（存在才显示） |
| `claude.legacy-statsig` | `statsig/` | 旧版本遗留统计缓存 | 无 | 是（存在才显示） |
| `claude.legacy-logs` | `logs/` | 旧版本遗留日志 | 无 | 是（存在才显示） |
| `claude.legacy-image-cache` | `image-cache/` | 旧版本遗留图片缓存（新版本改存系统临时目录） | 无 | 是（存在才显示） |
| `claude.mcp-logs` | 绝对路径：macOS `~/Library/Caches/claude-cli-nodejs/`；Windows `%LOCALAPPDATA%\claude-cli-nodejs\`；Linux `~/.cache/claude-cli-nodejs/` | MCP 服务器日志（历史长期占用大头） | 丢失历史 MCP 日志，下次会话重建 | 是 |

> 为什么 `projects/` 不删：它是会话记录（transcript）+ 项目级自动记忆，删了就无法 `--resume`、无法回看，属于不可逆信息损失 —— 归 B 类。

#### 4.2.2 Codex CLI

根：`$CODEX_HOME`（若设置）否则 `~/.codex`

| cacheId | 相对路径 | 内容 | 删除后影响 | 默认勾选 |
|---------|----------|------|-----------|---------|
| `codex.log` | `log/` | 运行日志（含 `codex-tui.log`） | 无 | 是 |
| `codex.tmp` | `tmp/` | 临时目录/临时工作区（存在才显示） | 无（若 10 分钟内有修改，按 R7 默认不勾选） | 是 |

#### 4.2.3 Cursor

根：macOS `~/Library/Application Support/Cursor`、Windows `%APPDATA%\Cursor`、Linux `~/.config/Cursor`；另 macOS `~/Library/Caches/Cursor/`

| cacheId | 相对路径 | 内容 | 删除后影响 | 默认勾选 |
|---------|----------|------|-----------|---------|
| `cursor.cache` | `Cache/` | Chromium HTTP 缓存 | 无（重新下载） | 是 |
| `cursor.cached-data` | `CachedData/` | 编译后的扩展/JS 缓存 | 首次启动略慢 | 是 |
| `cursor.code-cache` | `Code Cache/` | V8 字节码缓存 | 首次启动略慢 | 是 |
| `cursor.gpu-cache` | `GPUCache/` | GPU 渲染缓存 | 无 | 是 |
| `cursor.dawn-cache` | `DawnCache/`、`DawnGraphiteCache/`、`DawnWebGPUCache/` | WebGPU 缓存 | 无 | 是 |
| `cursor.shader-cache` | `ShaderCache/`、`GrShaderCache/` | 着色器缓存 | 无 | 是 |
| `cursor.logs` | `logs/` | 运行日志与历史归档 | 失去诊断日志 | 是 |
| `cursor.cached-extensions` | `CachedExtensions/`、`CachedExtensionVSIXs/` | 扩展编译产物与安装包副本 | 扩展重新编译/重新下载（离线时可能失败） | 是 |
| `cursor.syscache` | macOS 专用：`~/Library/Caches/Cursor/` | macOS 侧缓存目录 | 无 | 是（仅 macOS） |

> Cursor 真正的体积大头是 `User/workspaceStorage/`（每工作区 `.pack` 编辑历史，常见 10~40 GB），但它是**编辑历史**，v1 只展示、不清理（见 §10-D1）。

#### 4.2.4 Windsurf

根：macOS `~/Library/Application Support/Windsurf`、Windows `%APPDATA%\Windsurf`、Linux `~/.config/Windsurf`；另 macOS `~/Library/Caches/com.exafunction.windsurf/`

| cacheId | 相对路径 | 内容 | 删除后影响 | 默认勾选 |
|---------|----------|------|-----------|---------|
| `windsurf.cache` | `Cache/` | Chromium HTTP 缓存 | 无 | 是 |
| `windsurf.cached-data` | `CachedData/` | JS/扩展编译缓存 | 首次启动略慢 | 是 |
| `windsurf.code-cache` | `Code Cache/` | V8 字节码缓存 | 首次启动略慢 | 是 |
| `windsurf.gpu-cache` | `GPUCache/` | GPU 渲染缓存 | 无 | 是 |
| `windsurf.logs` | `logs/` | 运行日志归档 | 失去诊断日志 | 是 |
| `windsurf.cached-extensions` | `CachedExtensions/`、`CachedExtensionVSIXs/` | 扩展编译产物与安装包副本 | 扩展重新编译/下载 | 是 |
| `windsurf.syscache` | macOS 专用：`~/Library/Caches/com.exafunction.windsurf/` | macOS 侧缓存目录 | 无 | 是（仅 macOS） |

#### 4.2.5 GitHub Copilot（CLI）

根：`~/.copilot`

| cacheId | 相对路径 | 内容 | 删除后影响 | 默认勾选 |
|---------|----------|------|-----------|---------|
| `copilot.logs` | `logs/` | CLI 运行日志（存在才显示） | 无 | 是 |

> VS Code 内 Copilot 扩展的缓存与 VS Code 自身状态耦合，v1 不处理（只检测 `~/.copilot`）。

#### 4.2.6 Gemini CLI

根：`~/.gemini`。**v1 无 A 类白名单。**

| 只读展示 | 路径 | 原因 |
|---------|------|------|
| 根占用 | `~/.gemini` | 让用户知道占多少 |
| 会话/临时数据 | `tmp/` | 其中包含自动保存的会话快照，属不可逆信息损失 → B 类，仅展示 + 「打开目录」 |

### 4.3 绝对不触碰清单（跨工具汇总）

**通用（任何情况下都不删）**

1. 任何**文件**（v1 只删目录）。
2. 工具根目录本身、磁盘根、用户主目录、系统目录、回收站目录。
3. 符号链接 / junction 指向的目标。
4. 白名单之外的一切目录（**默认拒绝**，不是"默认允许再过滤"）。

**按工具（只读展示，绝不清理）**

| 工具 | 绝不触碰 | 里面是什么 |
|------|---------|-----------|
| Claude Code | `projects/`、`agent-memory/`、`file-history/`、`tasks/`、`paste-cache/`、`uploads/`、`dev-mods/`、`usage-data/`、`feedback/`、`feedback-bundles/`、`plugins/`（含 `.trash/`）、`skills/`（含 `.trash/`）、`agents/`、`commands/`、`rules/`、`output-styles/`、`jobs/`、`daemon/`、`history.jsonl`、`stats-cache.json`、`.credentials.json`、`.claude.json`、`settings.json` | 会话记录、记忆、检查点、插件、技能、任务、凭据、全局配置 |
| Codex CLI | `sessions/`、`archived_sessions/`、`skills/`、`prompts/`、`state_*.sqlite`（含 `-wal`/`-shm`）、`history.jsonl`、`config.toml`、`auth.json` | 会话 rollout、技能、状态库、提示词历史、配置、凭据 |
| Cursor | `User/workspaceStorage/`（`.pack`/`.idx`/`state.vscdb`）、`User/History/`、`User/globalStorage/`、`User/settings.json`、`User/keybindings.json`、`extensions/` | 编辑历史、检查点、全局设置、扩展 |
| Windsurf | 同 Cursor；另 `~/.codeium/`（账号 token + Cascade 记忆）、`User/globalStorage/` | 账号、记忆、配置 |
| GitHub Copilot | `~/.copilot` 下除 `logs/` 外全部（`config.json`、`lsp-config.json`、会话状态等） | 配置、会话、认证 |
| Gemini CLI | `~/.gemini` 下全部（v1 完全只读） | 凭据、配置、会话 |

---

## 5. 交互与页面

### 5.1 信息架构（单页）

```
[ 顶部汇总卡 ]
  已检测 4 个工具 · 可清理 3.2 GB · 另有 8.7 GB 会话与配置（不清理）
  [重新扫描]

[ 工具卡片 × N ]  （按可清理体积降序，无清理项的排后面）
  ┌ Claude Code                     可清理 2.1 GB ──────────┐
  │ [可能正在使用]                                          │
  │ 可清理缓存（勾选）                                      │
  │  ☑ debug/                    812 MB    调试日志 · 可重建 │
  │  ☐ shell-snapshots/          3 MB      可能正在使用      │
  │  ── 会话与配置（不清理）── 未计算  [计算占用] [打开目录] │
  │  projects/ · file-history/ · plugins/ …（仅名称，不可勾选）│
  └────────────────────────────────────────────────────────┘

[ 底部操作条（吸底）]
  已选 3 项 · 3.2 GB              [清理选中项]
```

### 5.2 主流程

1. **进入页面** → 自动扫描（骨架屏 + "正在扫描本机 AI 工具…"）→ 卡片逐个/整体出现。
2. **浏览** → 展开卡片查看每个 A 项路径、大小、一句话说明；B 类可「计算占用」（独立计算，不阻塞）。
3. **勾选** → A 类默认全选；吸底条实时显示已选数量与合计。
4. **清理** → 点击「清理选中项」→ 确认弹窗：
   - 标题「确认清理」；
   - 列出每一条**绝对路径** + 大小（超过 20 条时折叠显示"另有 N 项"）；
   - 警示文案：「这些缓存删除后**不可恢复**，工具会在下次使用时自动重建；会话记录、配置与账号不受影响。」；
   - 按钮：「取消」（默认焦点）/「清理 N 项 · X GB」（危险色）。
5. **执行** → 按钮进入 loading + 计时；全部结束后自动重扫。
6. **结果** → 顶部显示「已释放 X GB」；结果区列出成功项与失败项（失败给原因与建议，如「请先退出 Cursor 后重试」）；数字刷新为清理后状态。

### 5.3 必须覆盖的状态

| 状态 | 文案/表现 |
|------|----------|
| 扫描中 | 骨架卡片 + 「正在扫描…」；页面可滚动、可切换导航 |
| 无任何受支持工具 | 空态：「未检测到 AI 编码工具」+ 支持工具清单（6 个名称） |
| 检测到但无可清理项 | 卡片仍显示，标注「已检测到 · 暂无可清理缓存」，B 类可查看 |
| 单项读取无权限 | 该行标「无权限读取」，其余正常 |
| 清理执行中 | 吸底按钮禁用 + 计时器；卡片不可勾选 |
| 清理部分失败 | 结果区分「已清理」/「未完成」，失败项给出原因 |
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
// 只读：自动检测 + A 类扫描
scan_ai_caches() -> Result<ScanResult, String>

ScanResult {
  tools: Vec<ToolReport>,          // 仅包含检测根存在的工具
  total_cleanable_bytes: u64,
}
ToolReport {
  tool_id: String,                 // "claude" | "codex" | "cursor" | ...
  display_name: String,
  root_dir: String,                // 仅用于展示
  detected: bool,
  abnormal: Option<String>,        // R9 路径异常原因
  cleanable_bytes: u64,
  items: Vec<CacheItem>,           // A 类
  non_cache: Vec<DataSummary>,     // B 类，只含名称/说明，不含大小
}
CacheItem { cache_id, label, path, bytes, last_modified_ms, in_use, skipped: Option<String> }
DataSummary { label, path, note }

// 只读：按需计算某个工具 B 类总占用
measure_tool_data(tool_id: String) -> Result<u64, String>

// 写：清理（前端只传 id，绝不传路径）
struct CleanTarget { tool_id: String, cache_id: String }
clean_ai_caches(items: Vec<CleanTarget>) -> Result<CleanResult, String>

CleanResult {
  freed_bytes: u64,
  succeeded: Vec<{ tool_id, cache_id, path, bytes }>,
  failed: Vec<{ tool_id, cache_id, path, reason }>,
}

// 复用已有命令：open_path(path) 打开目录（只对扫描返回的路径）
```

**实现约束**

- 路径字符串由后端返回给前端**仅用于展示**；前端不得把它作为清理入参回传。
- 清理前必须重新解析一次白名单路径（防止扫描与清理之间目录被替换为符号链接）。
- 删除使用 Rust 原生 `fs::remove_dir_all`，不调用 shell/PowerShell。
- 扫描为大目录时不得阻塞 UI：命令需 `async` + `spawn_blocking`（沿用 `scan_disk_usage` 的既有模式）。
- 扫描不得保留完整文件树，只累加字节数，避免内存膨胀。

---

## 7. 验收标准（可测）

> 验证环境：macOS（主）与 Windows 各一台；测试夹具 = 用临时目录模拟各工具根（Rust 单测）+ 真机抽查。

| ID | 验收项 | 通过条件 / 验证方法 |
|----|--------|-------------------|
| AC-01 | 工具入口 | `/tools/ai-cache-clean` 出现在顶部导航与工具列表（macOS 与 Windows 均可见，不受 windowsOnly 限制）；访问后进入「最近使用工具」 |
| AC-02 | 自动检测 | 用夹具分别创建/删除工具根后刷新：列表**恰好**包含根存在的工具，顺序按可清理体积降序；未安装的工具不出现 |
| AC-03 | 环境变量根 | `CLAUDE_CONFIG_DIR=/tmp/fixture-claude`（内含 `debug/`）时，检测根切换为该目录，清理只作用于其中白名单项 |
| AC-04 | 体积准确性 | 每个 A 项显示字节数 = 目录内文件字节数之和；与 `du -sk`（Windows 用资源管理器/等效工具）偏差 ≤5%（按字节求和为准） |
| AC-05 | 扫描性能 | 6 工具 A 项合计 <5 GB、SSD 条件下，全量扫描 ≤5 s；扫描期间页面可滚动、可切换导航，不出现"未响应" |
| AC-06 | 分级展示 | B 类无勾选框；点击卡片内「计算占用」后在 ≤10 s 内显示 B 类总大小（大目录给出计算中状态），期间不影响 A 类已显示的数字 |
| AC-07 | 清理需确认 | 未在弹窗点「清理」前，磁盘零变化（取消后目录仍在）；弹窗列出**每一条绝对路径**与合计大小、含"不可恢复"警示 |
| AC-08 | 释放量 | 结果中 `freed_bytes` = 各成功项清理前字节数之和（误差 ≤1%）；顶部显示"已释放 X GB" |
| AC-09 | 清理后重扫 | 成功项对应目录不存在；卡片可清理量归零（允许 ≤1 MB 的立即重建）；汇总数字同步刷新 |
| AC-10 | 白名单拒绝 | 向 `clean_ai_caches` 传 `claude.projects`、`codex.sessions`、`../etc`、空字符串、未知 id → 全部返回错误，磁盘无变化（`cargo test`） |
| AC-11 | 路径越界拒绝 | 白名单项在规范化后不在工具根内（含 `..`、被替换为指向 `/tmp` 的链接）→ 拒绝并记录原因（`cargo test`，R2/R4） |
| AC-12 | 符号链接 | 白名单目录为 symlink/junction 时：不删除、不跟随，结果标注"已跳过：符号链接"，链接目标仍存在（夹具测试 + 手工） |
| AC-13 | B/C 类零影响 | 清理前后对所有 B/C 类目录与文件做文件数 + SHA-256 比对，**完全一致**（夹具测试：A/B/C 混合目录树，逐项断言） |
| AC-14 | 运行中保护 | 夹具中给 A 项目录写一个 mtime=now 的文件 → 该项显示「可能正在使用」且默认未勾选；用户手动勾选后仍可清理 |
| AC-15 | 单项失败隔离 | 用夹具构造必然失败项（父目录只读 / macOS `chflags uchg` 锁定文件 / Windows 独占打开文件）→ 该项失败并给出中文原因，其他项成功；结果区同时列出成功/失败（注：仅把目录设只读在 Unix 上未必失败，测试不能依赖它） |
| AC-16 | 根目录保护 | 任何情况下请求删除工具根（`~/.claude`、`~/.codex`、`~/.cursor` 等）都被拒绝（`cargo test`，R5） |
| AC-17 | 空态/异常态 | 无工具时显示空态文案；无权限目录显示"无权限读取"；两种情况均不出现白屏或未捕获异常 |
| AC-18 | 无网络 / 无提权 | 断网下全部功能可用；全程无网络请求代码路径；普通权限完成所有操作，无 UAC/提权弹窗 |
| AC-19 | 幂等 | 连续两次清理同一批：第二次成功且 `freed_bytes = 0`，无错误 |
| AC-20 | 隐私 | 清理结果不落盘（无历史文件、无 localStorage 路径记录）；代码中无上传/遥测调用 |

**质量门（沿用项目宪法）**：`npm run lint`、`cargo fmt --check`、`cargo clippy` 干净；`cargo test` 覆盖 AC-10/11/12/13/14/16；PR 需过 architect + senior-qa 审查。

---

## 8. 安全边界（逐条回答"删错怎么办"）

1. **清理前是否确认**：是，**始终**。每次清理都弹确认框，列出全部绝对路径与合计；默认焦点在「取消」；`Esc` 取消；不存在"跳过确认"的设置项。
2. **是否只删缓存类目录**：是。删除目标只能是 §4.2 白名单中的目录；其中每一项都满足"删除后无用户可见内容损失、可自动重建"；文件永不删除。
3. **会话记录/配置/凭据绝对不删清单**：见 §4.3（Claude `projects/`、`file-history/`、`plugins/`、`.credentials.json`、`.claude.json`；Codex `sessions/`、`state_*.sqlite`、`auth.json`；Cursor/Windsurf `User/workspaceStorage`、`User/globalStorage`、`~/.codeium/`；Gemini 全目录）。
4. **是否可恢复**：缓存不可恢复（会被工具重建，但不是用户数据）。因此文案必须写明"不可恢复"，且**不提供**"移到回收站"这种假可恢复选项（回收站不释放空间，与目标矛盾）。
5. **误删防线（四层）**：①后端白名单（前端无法指定路径）②路径规范化 + 必须在工具根内 ③符号链接/根目录/越界拒绝 ④确认弹窗列路径。任一层失败都拒绝执行。
6. **权限模型**：不需管理员权限；遇到权限不足只标记该项失败，不尝试提权。
7. **并发安全**：清理前重新校验目录属性；若目录在扫描后被替换为符号链接 → 跳过。
8. **隐私**：不读取目录内容（只做元数据遍历统计大小），不读取文件名以外的内容；不联网、不落盘。

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

## 10. 待决策项（仅 2 项，均附推荐）

### D1（重要）Cursor / Windsurf 的 `workspaceStorage` 是否纳入 v1？

- **现状**：v1 只读展示、不清理。原因是 `.pack`/`.idx`/`state.vscdb` 是**每工作区的编辑历史与检查点恢复数据**，删除后该工作区的本地撤销/恢复能力丢失，不属于"无用户可见损失"。
- **代价**：这是 IDE 类工具最大的体积来源（常见 10~40 GB）。不纳入 → 工具对 Cursor/Windsurf 用户只能清出几百 MB～几 GB。
- **若纳入**需要：逐工作区列表（显示最后使用时间 + 大小）、默认只勾选"30 天未使用"、额外一次"将丢失这些工作区的本地编辑历史"确认、CI 增加测试。粗估 +1~1.5 人日。
- **推荐**：**v1 不纳入**，先上线安全版本；观察用户反馈后放 v1.1。理由：本工具的卖点是"敢让用户闭眼点"，第一版不碰编辑历史。

### D2（确认项）删除方式：直接删除（永久）还是移到回收站？

- **现状（推荐）**：直接永久删除。因为目标是"释放磁盘空间"，移到回收站不释放空间，还得再清空回收站，多一步且更让人困惑。
- **反向选择**：若用户更看重"可撤销"，可改为移到回收站，但需明确提示"空间未释放，清空回收站后生效"。
- **推荐**：保持直接删除 + 强确认弹窗。

---

## 11. 交付与工作范围粗估

- 分支：`feat/ai-cache-clean` → PR 到 `main`；提交遵循 Conventional Commits + 中文摘要。
- 改动面：`src-tauri/src/lib.rs`（2 个新命令 + 白名单表 + 测试）、`src/lib/tools.ts`（注册入口）、`src/app/tools/ai-cache-clean/page.tsx`（新页面）、`src/lib/tauri.ts`（封装）。
- 粗估：Rust 1.5 人日、前端 1.5 人日、测试/联调 1 人日（不含评审与修复）。
- 建议顺序：白名单 + 扫描（含测试）→ 页面 → 清理 + 确认 → 结果态与真机验证。

---

## 附录 A：路径依据

- Claude Code `~/.claude` 目录与保留策略（含"删了会怎样"表）：[Explore the .claude directory](https://code.claude.com/docs/en/claude-directory)（官方文档，`cleanupPeriodDays`、application data、state files to keep 等小节）
- Codex CLI 配置与 `CODEX_HOME`：[openai/codex docs/config.md](https://github.com/openai/codex/blob/main/docs/config.md)
- GitHub Copilot CLI 用户目录 `~/.copilot`（含 `lsp-config.json` 等）：[github/copilot-cli README](https://github.com/github/copilot-cli)
- Cursor / Windsurf 为 VS Code 派生（Electron），`Cache` / `CachedData` / `Code Cache` / `GPUCache` / `CachedExtensions` / `logs` 为其通用缓存目录；`User/workspaceStorage`、`User/globalStorage`、`~/.codeium` 为用户数据。
- Gemini CLI `~/.gemini/tmp` 承载项目哈希下的临时/会话快照数据 —— 因此 v1 归入 B 类。

> 白名单按本规格固化为常量。若某工具后续版本改名/新增缓存路径，**必须走规格变更**（追加路径 + 测试），禁止运行时"猜目录"或通配匹配。

## 附录 B：术语

- **A 类缓存**：删除后无用户可见内容损失、工具可自动重建的目录。
- **B 类用户数据**：会话记录、编辑历史、插件/技能/记忆、配置等，删除即信息损失。
- **C 类**：凭据、账号、根目录、白名单外一切，永不触碰。
- **可能正在使用**：目录内 10 分钟内有文件修改 → 该项默认不勾选。
