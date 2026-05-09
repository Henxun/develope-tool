# DevToolkit Desktop

使用 `Tauri + React + Next.js + TailwindCSS` 实现的开发工具客户端示例。

## 功能

- 压缩工具：支持 `zip`、`tar`、`tar.gz`、`tar.bz2`、`tar.xz`
- 解压工具：按压缩包扩展名自动识别格式并解压
- 过滤条件：支持 Glob 规则排除文件/目录（如 `node_modules/**`、`*.log`）
- 原生路径选择器：支持选择源文件/文件夹、压缩包保存位置、解压目录
- 路由化工具导航：工具独立页面、当前路由高亮、最近使用入口
- Windows 程序迁移：迁移安装目录并更新注册表、环境变量；快捷方式仅做检测提示（避免执行 PowerShell 脚本风险，仅 Windows 显示）
- 安全策略：迁移参数做严格校验（绝对路径、非法字符、白名单前缀），命令执行使用 Rust 原生 API
- 权限策略：默认仅更新用户级注册表（HKCU）；可手动启用 HKLM 更新（需管理员权限）
- 提权支持：启用 HKLM 后可一键“提权并重启”，以管理员权限继续迁移
- 自动检测：选择安装目录后自动扫描并展示关联注册表、快捷方式、环境变量信息
- 工具数据迁移：支持将 `.claude` 等目录迁移到其它盘，可选软链接/环境变量方案
- 获取本机系统信息（OS、架构、工作目录、NODE_ENV）
- 浏览本地目录（通过 Tauri Rust 命令读取）
- 桌面端 UI（Next.js App Router + TailwindCSS 4）

## 技术栈

- 前端：Next.js 16、React 19、TailwindCSS 4
- 桌面容器：Tauri 2
- 后端能力：Rust Commands（`get_system_info` / `list_directory`）
- 后端能力：Rust Commands（含 `compress_archive` / `extract_archive`）

## 运行方式

1. 安装 Node.js 依赖

```bash
npm install
```

2. 安装 Rust（若未安装）

- Windows 推荐使用 `rustup`
- 同时确保 Tauri 系统依赖已安装（WebView2 等）

3. 启动桌面开发环境

```bash
npm run tauri:dev
```

## 构建

```bash
npm run tauri:build
```

## 关键目录

- `src/app`：Next.js 前端页面
- `src/app/tools/archive/page.tsx`：压缩/解压工具页面
- `src/app/tools/system/page.tsx`：系统信息与目录浏览页面
- `src/app/tools/windows-migrate/page.tsx`：Windows 程序迁移页面
- `src/app/tools/tool-data-migrate/page.tsx`：工具数据迁移页面
- `src-tauri/src/lib.rs`：Tauri Rust 命令实现
- `src-tauri/tauri.conf.json`：Tauri 配置
