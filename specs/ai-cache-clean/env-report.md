# 环境评估报告 — develope-tool / ai-cache-clean

- 执行人：Ops（运维）
- 时间：2026-10-31（Asia/Shanghai）
- 状态：**已执行安装**（用户批准后完成，实测记录见 §0.5）。§1–§7 是最初的只读勘察结论，保留作为决策依据。
- 约束遵守：**零 sudo、零全局 PATH 改动、零 shell 配置改动、零 git 工作区改动**。工具链全部落在 `/Users/henxun/.dsh/bot/workspace/.ops/`（workspace 根，**在项目仓库之外**，故仓库无任何新增忽略规则需要）。

---

## 0. 结论速览（安装完成后实测，2026-10-31 16:00）

| 验证目标 | 现在能否跑 | 实测结果 |
|---|---|---|
| 依赖安装 | ✅ | `npm ci`：400 包，0 error，`node_modules` 466 MB |
| `npm run lint` | ✅ | **exit 0，零告警** |
| `npm run build`（Next 16 静态导出） | ✅ | **exit 0**，`out/` 1.8 MB，11 个静态页（含 `/tools/ai-cache-clean`） |
| `cargo -V` / `rustc -V` / `rustfmt` | ✅ | cargo/rustc **1.99.1**，rustfmt 1.10.0-stable |
| `cargo metadata` 读 `src-tauri/Cargo.toml` | ✅ | OK（534 crates / lockfile v4 可解析） |
| `cargo fmt --check` / `clippy` / `test` | ✅ 工具就绪，**尚未执行** | 按指示等 Coder 交完再全量编译 |
| `npm run tauri:build` | ⏸ 待首次编译 | tauri-cli **2.10.1** 已就绪，`npm` 可从 PATH 解析（`beforeBuildCommand` 前置条件已满足） |

**一句话：本机现在前后端都能验证了。** 前端已完整跑通（lint + build + 静态产物）；Rust 工具链就绪、能正确解析 manifest，只差首次全量编译（预计 4–8 分钟）。

### 怎么用（队友请照这个来）

```bash
cd /Users/henxun/.dsh/bot/workspace/develope-tool
source /Users/henxun/.dsh/bot/workspace/.ops/env.sh   # 只在当前 shell 生效，不写任何配置

npm run lint && npm run build          # 前端
cargo fmt    --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml
cargo test   --manifest-path src-tauri/Cargo.toml
npm run tauri:dev                      # 需要开窗时
```

`env.sh` 做的事：把 `.ops/node/bin`（官方 node）、`.ops/node-tools/bin`、`.ops/rust/cargo/bin` 加到**当前 shell** 的 PATH，并设置 `RUSTUP_HOME` / `CARGO_HOME` / `npm_config_cache` 指向 `.ops/` 内。

### 安装后本机新增了什么（全部在 `.ops/`，可整目录删除回滚）

| 路径 | 内容 | 体积 |
|---|---|---|
| `.ops/node/` | 官方 Node.js v24.21.0 darwin-arm64（含 npm 11.19.0） | 199 MB |
| `.ops/node-tools/` | 自举的 npm 12.2.1 + `npm`/`npx` shim（备用） | 17 MB |
| `.ops/rust/` | rustup 1.29.2 + Rust 1.99.1（rustc/cargo/rust-std/rustfmt/clippy） | 485 MB |
| `.ops/npm-cache/` | npm 缓存（400 包，加速重建） | 120 MB |
| `.ops/env.sh` | 环境激活脚本 | 4 KB |
| **合计** | | **820 MB** |

---

## 0.5 执行记录（真实输出）

### 1) Node / npm

```
node   v24.21.0
npm    11.19.0
```

> ⚠️ 这里踩了一个坑，值得记下来：**DSH 自带的 node（`~/.dsh/dsh-runtimes/.../node/bin/node`）不能用来 build**。它是用 hardened runtime + library validation 签名的（没有 `com.apple.security.cs.disable-library-validation` entitlement），加载 Next 的 `@next/swc-darwin-arm64/next-swc.darwin-arm64.node` 时报：
> ```
> code signature ... not valid for use in process:
> mapping process and mapped file (non-platform) have different Team IDs
> ```
> 官方 Node.js 发行版带 `disable-library-validation: true`，所以下载官方 node 后一次通过。**这也是为什么 `.ops/node` 必须存在、且优先于 DSH node。**

### 2) Rust

```
cargo   cargo 1.99.1 (5f94df478 2026-08-27)
rustc   rustc 1.99.1 (58cf8f9c3 2026-10-13)
rustfmt rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13)
rustup  1.29.2 (d8c92c676 2026-10-21)

已装组件: cargo / clippy / rust-std / rustc / rustfmt (aarch64-apple-darwin)
安装耗时: 22 秒（USTC 镜像）
RUSTUP_HOME = .ops/rust/rustup
CARGO_HOME  = .ops/rust/cargo
```

`cargo --manifest-path src-tauri/Cargo.toml --version` 这个写法本身不成立（cargo 的 `--version` 不接受 `--manifest-path`，会报 `unexpected argument`）。用等价的轻量检查替代：

```
$ cargo locate-project --manifest-path src-tauri/Cargo.toml
{"root":"/Users/henxun/.dsh/bot/workspace/develope-tool/src-tauri/Cargo.toml"}

$ cargo metadata --no-deps --format-version 1 --manifest-path src-tauri/Cargo.toml
exit=0  →  package: devetool 0.1.0 / edition 2021
        →  targets: staticlib, cdylib, rlib / bin / custom-build
```
**未做任何编译**（按指示留给 Coder 交付后）。首次全量编译预估：`cargo check` 2–4 min、`cargo test` 4–8 min、`target/` 2–4 GB。

### 3) 前端

```
$ npm ci
added 400 packages in 2s        # 缓存已暖
npm warn install-scripts  2 packages had install scripts blocked (sharp, unrs-resolver)

$ npm run lint
> eslint
exit=0                          # 零告警

$ npm run build
▲ Next.js 16.2.1 (Turbopack)
✓ Compiled successfully in 1026ms
  Finished TypeScript in 778ms
✓ Generating static pages (11/11)
Route (app): /  /_not-found  /tools  /tools/ai-cache-clean  /tools/archive
             /tools/disk-heatmap  /tools/system  /tools/tool-data-migrate  /tools/windows-migrate
exit=0

$ du -sh out
1.8M    out
```

### 4) 执行过程中额外踩到的两个坑（已绕过，记录备查）

| 坑 | 现象 | 处理 |
|---|---|---|
| DSH 文件沙箱只允许写 workspace | `npm ci` 报 `EPERM ... ~/.npm/_cacache/CACHEDIR.TAG`（错误信息误导成"root-owned files"） | `npm_config_cache` 指到 `.ops/npm-cache`，零 sudo |
| npm 12 默认 `allow-remote = none` | lockfile 里有 **41 个 resolved 指向 `registry.npmmirror.com`**（另 442 个是 npmjs），npm 12 拒绝下载 remote tarball：`EALLOWREMOTE Refusing to fetch use-debounce@https://registry.npmmirror.com/...` | 用 `--allow-remote=all` 装（integrity 仍在校验）；日常改用官方 npm 11.19.0，无此限制 |
| ESLint 会扫到工具链目录 | `.tools/npm/lib/**` 被 lint 出 690 个 error | 把工具链整体移到**项目目录外**（`.ops/`），项目保持干净，`.gitignore` 无需改动 |

> 附注：`package-lock.json` 里混着 npmmirror 源，说明之前有人在 npmmirror 环境装过依赖。现在不影响构建，但**如果 CI 或队友换 npm 12 会再次撞 EALLOWREMOTE**，建议后续统一一次（另开任务，不阻塞本 feature）。

---


## 1. 机器画像（实测）

| 项 | 值 |
|---|---|
| 机型 | Mac mini（Apple M6，12 核） |
| 内存 | 16 GB |
| 架构 | arm64（原生，**无 Rosetta** 进程） |
| macOS | **27.2**（Build 26B123），内核 Darwin 27.2.0 |
| 磁盘可用 | **178 GiB**（`/` 与 Data 卷均充足） |
| 当前用户 | `henxun`（uid 501，属于 `admin` 组，但本次操作一律不用 sudo） |
| Shell | `/bin/zsh` |

### 系统自带开发工具（够用）

| 工具 | 版本 / 位置 | 对本项目的意义 |
|---|---|---|
| Xcode CLT | `/Library/Developer/CommandLineTools`，CLT 27.2.0.0 | Rust 链接必需，**已具备** |
| macOS SDK | `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`（27.2，另有 26/26.5/27/27.2 多份） | **已具备** |
| clang | Apple clang 21.0.0, target `arm64-apple-darwin27.2.0` | `cc` crate 编译 C 依赖（bzip2-sys、lzma-sys）用，**已具备** |
| swift | 6.4.2（CLT 内） | 本项目不需要 |
| git | 2.54.0 (Apple Git-157) | ✅ |
| python3 | 系统 3.9.6；conda 3.14.9 | 可用于脚本 |
| conda | 26.9.4，`~/miniconda3`（1.2 GiB），channel 只有 `defaults` | 可选装包途径，见 §5 |
| 完整 Xcode | ❌ 无（`xcodebuild` 不可用，仅 CLT） | 本项目不需要完整 Xcode |
| Homebrew | ❌ **无**（`/opt/homebrew`、`/usr/local/Cellar` 均不存在） | 排除 brew 路线 |
| Docker / podman / colima / lima | ❌ 全无 | 排除容器化验证 |

### 网络实测（决定耗时估计）

| 目标 | 实测吞吐 | 备注 |
|---|---|---|
| `registry.npmjs.org`（next 包） | **≈ 1.8 MB/s** | 可用 |
| `registry.npmmirror.com` | **≈ 9.8 MB/s** | 明显更快，可作加速 |
| `static.rust-lang.org`（官方 Rust 分发） | **22–56 KB/s** | ⚠️ 极慢，157 MiB 要 45–120 分钟 |
| `mirrors.ustc.edu.cn/rust-static` | **≈ 5.5 MB/s** | ✅ **快 100 倍，推荐** |
| `index.crates.io`（sparse index） | ≈ 52 KB/s | 可用但慢 |
| `mirrors.ustc.edu.cn/crates.io-index` | ≈ 369 KB/s | 推荐 |
| `static.crates.io`（crate 下载） | 183–257 KB/s | 可用 |

DNS/HTTPS 全通，无代理需求。

---

## 2. 工具链核查明细（逐项证据）

### 2.1 当前 PATH

```
/Users/henxun/.pi/agent/bin
/usr/local/bin                                  ← 该目录不存在（死条目）
/System/Cryptexes/App/usr/bin
/usr/bin:/bin:/usr/sbin:/sbin
/var/run/.../cryptexd/... (3 条，系统 cryptex)
/pki/env/global/bin                             ← 实际为 /pkg/env/global/bin，是空符号链接（死条目）
```

`~/.zshrc` 只有三段：`.pi/agent/bin`、conda init、`~/.local/bin`。其中 `~/.local/bin` 只放了一个 `ego-browser` 符号链接。

### 2.2 Node.js —— **系统没有，但 DSH 自带一份可用的**

```
node / npm / npx / pnpm / yarn   → 全部不在 PATH
/usr/bin/node                    → 不存在
~/.nvm  ~/.volta  ~/.fnm  ~/n  ~/.bun  ~/.deno  ~/.asdf  ~/.proto  → 全部不存在
/usr/local/bin                   → 目录都不存在
conda bin 内                     → 无 node/npm
```

**但发现：DeepSeek Harness 自带完整 Node 运行时**

| 项 | 路径 / 值 |
|---|---|
| node | `/Users/henxun/.dsh/dsh-runtimes/dsh-primary-runtime/dependencies/node/bin/node` |
| 版本 | **v24.21.0**（`darwin arm64`，实测可执行） |
| pnpm | `/Users/henxun/.dsh/dsh-runtimes/dsh-primary-runtime/dependencies/pnpm/bin/pnpm.mjs` |
| 版本 | **11.28.5**（实测可执行） |
| npm | ❌ **不存在**（该 node 是单二进制分发，`lib/node_modules` 里只有 README，无 npm、无 corepack） |
| pnpm store | 默认落在 `/Users/henxun/.dsh/bot/workspace/.pnpm-store/v11`（在 workspace 内，可写） |

> 另有 `/Applications/DeepSeek Harness.app/Contents/Resources/runtime/bin/node`，但它是个 100 字节的壳脚本（调用外部路径，实测报 `exec: not found`），不用它。
> `Qoder CN.app` / Harness 都是 Electron，理论上可用 `ELECTRON_RUN_AS_NODE=1` 跑 JS，但版本不可控，**不建议**用于构建。

### 2.3 Rust —— **完全没有**

```
cargo / rustc / rustup   → 不在 PATH
~/.cargo  ~/.rustup      → 不存在
/opt/homebrew            → 不存在
/usr/local/{bin,Cellar,opt} → 不存在
/Applications 全盘（深度 6）搜 node/cargo/rustc → 无 Rust
~/Library、/opt、/pkg 深度 4 搜索 → 无
conda bin                → 无
```

结论：Rust 侧是**零基础**，必须安装才能跑 `cargo`。

---

## 3. 项目构建剖析

### 3.1 前端

| 项 | 值 |
|---|---|
| Next.js | 16.2.1（`engines.node: >=20.9.0`） |
| 输出模式 | `next.config.ts` 里 `output: "export"` → `next build` 直接产出 `out/` |
| 依赖清单 | `package-lock.json`，lockfileVersion 3，**483 个包**（含 81 个平台可选包） |
| 脚本 | `dev / build / start / lint / tauri / tauri:dev / tauri:build` |
| 现状 | `node_modules` **不存在**（未安装过）、`out/`、`.next/` 均不存在 |

**依赖体积（用 registry 元数据实测，未下载）**

| 口径 | 体积 |
|---|---|
| 本机 darwin-arm64 实际会装（467 包已测） | **≈ 435 MiB** 解压后 |
| 会被跳过的平台变体（linux/win32/darwin-x64/freebsd/android） | ≈ 1334 MiB |
| 最大三包 | `next` 147.1 MiB、`@next/swc-darwin-arm64` 116.0 MiB、`typescript` 22.5 MiB |

→ 压缩包下载量估 **150–200 MB**，npm 官方源 1.8 MB/s 下约 **1.5–2 分钟**。

### 3.2 Rust / Tauri

| 项 | 值 |
|---|---|
| Tauri | `tauri 2.10.3`、`tauri-build 2.5.6`、`wry 0.54.4`、`tao 0.34.8` |
| `Cargo.lock` | **lockfile v4**（要求 cargo ≥ 1.78）→ 当前 Rust stable 1.99.1 满足 |
| 锁定 crate 数 | **534 个** |
| 平台相关 | `winreg` / `windows-sys` 有 `#[cfg(windows)]` 门控，macOS 上不编译 |
| 需 C 编译的依赖 | `bzip2-sys`、`lzma-sys`（xz2）、`libz` → 依赖 clang ✅ 已具备 |
| `src-tauri/target` | 不存在（从未编译） |
| 已实现度 | `grep clean_ai_caches` 无命中 → **本 feature 尚未开始实现**，环境评估正好在动手前做 |

### 3.3 ⚠️ 关键坑：`tauri.conf.json` 硬编码了 `npm`

```json
"beforeBuildCommand": "npm run build",
"beforeDevCommand": "npm run dev",
"frontendDist": "../out"
```

本机没有 `npm` 可执行文件。若批准走 "DSH node + pnpm" 路线，`pnpm tauri build` 仍会在前端构建阶段因为找不到 `npm` 而**失败**。三种解法（按侵入性排序）：

1. **给 npm 做个 PATH 前置 shim**（推荐，最干净）：在 workspace 下放 `./.bin/npm` 包装脚本指向自举出来的 `npm-cli.js`，命令前 `PATH="$PWD/.bin:$PATH"` 临时生效，**不写任何 shell 配置**。
2. 自举一份真 npm（下载 `npm-12.2.1.tgz`，**3.06 MB**，解压约 20 MiB），跑 `node npm/bin/npm-cli.js` —— 同样只放 workspace 内。
3. 改 `tauri.conf.json` 的 `beforeBuildCommand` 为 `pnpm run build` —— **不推荐**，会进 git、影响 CI 与队友。

---

## 4. 验证矩阵（安装后状态）

> 下表已按 **2026-10-31 安装完成后**的实际状态更新；详细实测输出在 §0.5。

| # | 命令 | 状态 | 备注 |
|---|---|---|---|
| 1 | `source .ops/env.sh` | ✅ | 一条命令激活全部工具链，不改任何配置 |
| 2 | `npm ci` | ✅ 已跑通 | 400 包 / 466 MB / 0 error |
| 3 | `npm run build`（Next 静态导出 → `out/`） | ✅ 已跑通 | exit 0，`out/` 1.8 MB，11 页 |
| 4 | `npm run lint` | ✅ 已跑通 | exit 0，零告警 |
| 5 | `cargo fmt --check` | ✅ 可用 | 工具就绪，等 Coder 交付后跑 |
| 6 | `cargo clippy` | ✅ 可用 | 首次编译 2–4 min |
| 7 | `cargo test` | ✅ 可用 | 首次编译 4–8 min |
| 8 | `npm run tauri:build`（.app/.dmg） | ⏸ 待首次编译 | tauri-cli 2.10.1 + npm 均可解析；产物未签名，仅本机可用 |

### 验证保真度说明

依赖是用 **`npm ci` + `package-lock.json`** 装的（不是 pnpm），与 CI 的 `npm ci` 完全一致 —— 锁文件未被修改（`git status` 里 `package-lock.json` 无变化），版本不会漂移。

---

## 5. 安装途径对比（当时的决策依据；**实际执行的是 5.A 官方 node + 5.B 方案 B0**）

### 5.A Node.js

| 方案 | 大小 | sudo | 预计耗时 | 失败风险 | 侵入性 |
|---|---|---|---|---|---|
| **A0 用 DSH 内置 node + pnpm（推荐）** | **0** | 无 | **0** | 低。风险：无 npm，需自举或 shim；DSH 升级可能换路径 | 零 |
| A1 官方 tar.xz 解压到 workspace 内 | 27.4 MB 下载，解压约 120 MiB | 无 | 1–2 分钟 | 低 | 低（只在 workspace） |
| A2 官方 `.pkg` 安装器（93.8 MB） | 93.8 MB | **要**（装到 `/usr/local`） | 3–5 分钟 | 中：写系统目录，卸载麻烦 | 高 |
| A3 Homebrew 装 node | brew 自身 500 MB+ | **要** | 20–40 分钟 | 高：macOS 27.2 太新，brew 配方未必适配；本机无 brew | 很高 |
| A4 `conda install -c conda-forge nodejs` | ~30 MB | 无（进 `~/miniconda3`） | 1–2 分钟 | 中：需加 conda-forge 源，node 版本非官方编排 | 中 |

**结论：不需要装 Node。** 前端用 A0 就能验证。只有当你要"长期、与 DSH 解耦的 Node 环境"时才考虑 A1。

### 5.B Rust（这是真正的决策点）

| 方案 | 下载 / 落盘 | sudo | 预计耗时 | 失败风险 | 侵入性 |
|---|---|---|---|---|---|
| **B0 rustup + USTC 镜像，装进 workspace（推荐）** | 下载 **157 MiB**；落盘 **1.2–1.4 GiB** | **无** | 下载 ~30 秒 + 安装 1–2 分钟 | 低。风险：路径需按命令 inline 传，不能全局 `cargo` | **零**（`RUSTUP_HOME`/`CARGO_HOME` 都指向 workspace，不写 shell 配置） |
| B1 rustup 官方脚本（默认路径 `~/.cargo`+`~/.rustup`） | 同上，但官方源 22–56 KB/s | 无 | **45–120 分钟**（除非换镜像） | 中：默认会改 `~/.zshrc`（可加 `--no-modify-path` 规避） | 中 |
| B2 纯 tarball 解包（rustc+cargo+rust-std+rustfmt+clippy 五个包 → workspace/.toolchain） | 同上 | 无 | 下载 ~30 秒 + 解包 30 秒 | 低。风险：无 rustup 元数据，手工维护版本/组件 | **零**（比 B0 更裸） |
| B3 Homebrew 装 rust | brew + rustup 双份 | **要** | 30–60 分钟 | 高 | 很高 |
| B4 `conda install -c conda-forge rust` | ~400 MB | 无 | 5–10 分钟 | 中高：conda 的 rust 常缺 `rust-std` 目标/组件错配，Tauri 交叉编译易踩坑 | 中 |
| B5 只走 CI（GitHub Actions，已有 `build.yml`，`macos-latest`） | 0 | 无 | 每次 10–20 分钟，且要 push | 低 | 零，但**本地迭代极慢**，且看不到编译期细节 |

**rustup 组件明细（USTC 镜像实测大小）**

| 组件 | 大小 | 是否需要 |
|---|---|---|
| `rustup-init` | 11.3 MB | B0 需要 |
| `rustc` | 85.3 MB | ✅ |
| `cargo` | 13.1 MB | ✅ |
| `rust-std` (aarch64-apple-darwin) | 48.5 MB | ✅ |
| `rustfmt` | 2.2 MB | ✅ 质量门要 `cargo fmt --check` |
| `clippy` | 4.4 MB | ✅ 质量门要 `cargo clippy` |
| `rust-docs` | 81.7 MB | ❌ 跳过（用 `--profile minimal` 而非 `default`，`default` 会白拉这 82 MB） |
| **合计（不含 docs）** | **≈ 157 MiB** | 当前 Rust stable **1.99.1**（2026-10-15） |

### 5.C 首次 Rust 编译成本（装完工具链后，一次性）

| 项 | 估计 |
|---|---|
| crates 下载 | 534 个 crate，**80–150 MB**；crates.io 183–257 KB/s → 6–14 分钟；配 USTC crates 镜像 → 2–4 分钟 |
| `cargo check` 首次 | 2–4 分钟（M6 12 核） |
| `cargo test` 首次 | 4–8 分钟 |
| `cargo build`（release，产 dmg） | 8–15 分钟 |
| `src-tauri/target` 落盘 | 2–4 GiB（`.gitignore` 已忽略） |

---

## 6. 执行路线（**已执行**，保留备查；命令里的 `.tools/`/`.toolchain/` 实际落在 `.ops/node-tools`、`.ops/rust`）

### 第 0 步 —— 现在就能做，但需你确认「装依赖算不算安装」

```bash
# 全部使用 DSH 自带 node，不写任何配置，只在命令前 inline PATH
NODE=/Users/henxun/.dsh/dsh-runtimes/dsh-primary-runtime/dependencies/node/bin/node
# 自举 npm（3.06 MB，解压到 workspace/.tools/npm）
TMP=$(mktemp -d); curl -sSL https://registry.npmjs.org/npm/-/npm-12.2.1.tgz | tar -xz -C "$TMP"
mkdir -p .tools && mv "$TMP/package" .tools/npm
# 装依赖（等价 npm ci，保真走 package-lock.json）
"$NODE" .tools/npm/bin/npm-cli.js ci --no-audit --no-fund
# 验证前端
"$NODE" .tools/npm/bin/npm-cli.js run lint
"$NODE" .tools/npm/bin/npm-cli.js run build     # → out/
```

产出：`out/` 静态站点（验证 Next 16 静态导出的真实产物）。
副作用：项目内多出 `.tools/npm`（约 20 MiB）、`node_modules/`（435 MiB）、`out/`；配置文件零改动；`package-lock.json` 不变（`npm ci` 只读它）。

### 第 1 步 —— 需批准：装 Rust（方案 B0，零 sudo、零全局配置）

```bash
export RUSTUP_HOME="$PWD/.toolchain/rustup"
export CARGO_HOME="$PWD/.toolchain/cargo"
export PATH="$CARGO_HOME/bin:$PATH"
export RUSTUP_DIST_SERVER=https://mirrors.ustc.edu.cn/rust-static   # 官方源只有 22–56KB/s
curl -sSf https://mirrors.ustc.edu.cn/rust-static/rustup/dist/aarch64-apple-darwin/rustup-init -o /tmp/rustup-init
sh /tmp/rustup-init -y --no-modify-path \
  --profile minimal --component rustfmt --component clippy \
  --default-toolchain 1.99.1
```

（用 `minimal` + 两个组件，避开 `default` 里 82 MB 的 `rust-docs`。）

关键点：`--no-modify-path` 保证**不碰 `~/.zshrc`**；`RUSTUP_HOME`/`CARGO_HOME` 指到项目内，`~/.cargo`、`~/.rustup` 不会被创建。三条 export 只在执行命令的那条 shell 里有效，不写入任何文件。

### 第 2 步 —— 验证 Rust 侧（连同质量门）

```bash
M=src-tauri/Cargo.toml
cargo fmt    --manifest-path $M --check
cargo clippy --manifest-path $M
cargo test   --manifest-path $M
```

### 第 3 步 —— 完整打包（可选）

`tauri.conf.json` 的 `beforeBuildCommand` 写死了 `npm run build`，必须先让 `npm` 可解析。做一个 workspace 内的 shim（纯脚本文件，不改任何全局配置）：

```bash
mkdir -p .tools/bin
cat > .tools/bin/npm <<'SH'
#!/bin/sh
exec "$NODE" "$PWD/.tools/npm/bin/npm-cli.js" "$@"
SH
chmod +x .tools/bin/npm
PATH="$PWD/.tools/bin:$PATH" "$NODE" .tools/npm/bin/npx-cli.js tauri build
```

⚠️ 打包出来的 `.app` **未签名未公证**（本机无 Apple Developer 证书），只能本机运行，不能对外分发。正式发布走 `.github/workflows/build.yml`（tag 触发）。

---

## 7. 风险与回滚

| 风险 | 影响 | 回滚方式 |
|---|---|---|
| 工具链被误提交 | 仓库变大 | **已规避**：工具链在 `.ops/`（项目仓库之外），仓库内原有 `.gitignore` 已忽略 `node_modules/`、`target/`、`out/`；`git status` 确认无我的改动 |
| `npm ci` 失败（网络/校验） | 前端无法验证 | `rm -rf node_modules`，无副作用 |
| rustup 安装失败 | Rust 仍不可用 | `rm -rf .ops/rust`，无副作用（未碰 `~/` 与系统） |
| 首次 `cargo build` 磁盘吃紧 | 178 GiB 可用，风险低 | `cargo clean` 或删 `src-tauri/target`（约 2–4 GB） |
| Tauri 产物未签名 | 只能本机跑 | 不对外分发；正式发布交 CI（`.github/workflows/build.yml`） |
| DSH 升级后 `.ops/env.sh` 里的 DSH node 路径失效 | 兜底 node 找不到 | 主用 node 已是官方版（`.ops/node`），不受影响；改一行就能修 |

**完整回滚（一条命令，恢复到安装前）：**

```bash
rm -rf /Users/henxun/.dsh/bot/workspace/.ops
rm -rf /Users/henxun/.dsh/bot/workspace/develope-tool/node_modules \
       /Users/henxun/.dsh/bot/workspace/develope-tool/out \
       /Users/henxun/.dsh/bot/workspace/develope-tool/.next
```

系统层面：**零残留**（未装任何 .pkg、未写 `/usr/local`、未建 `~/.cargo`/`~/.rustup`、未改 `.zshrc`）。

---

## 8. 执行清单（做了什么 / 没做什么）

**已执行（用户批准后）：**

| 动作 | 位置 | 是否可回滚 |
|---|---|---|
| 下载官方 Node.js v24.21.0 | `.ops/node/`（199 MB） | ✅ 删目录 |
| 自举 npm 12.2.1 + `npm`/`npx` shim | `.ops/node-tools/`（17 MB） | ✅ 删目录 |
| 安装 Rust 1.99.1（minimal + rustfmt + clippy，USTC 镜像） | `.ops/rust/`（485 MB） | ✅ 删目录 |
| `npm ci` 装前端依赖（400 包） | 项目 `node_modules/`（466 MB） | ✅ 删目录（已 gitignore） |
| `npm run lint` / `npm run build` | 生成 `out/`（1.8 MB） | ✅ 删目录（已 gitignore） |
| 写 `.ops/env.sh` 激活脚本 | `.ops/env.sh` | ✅ 删文件 |

**没做：**
- ❌ 未使用 sudo
- ❌ 未修改全局 PATH、`~/.zshrc`、`/etc/paths.d`、`~/.cargo`、`~/.rustup`（均已复核不存在/未变）
- ❌ 未跑任何 `cargo build` / `cargo test` / `tauri build`（按指示等 Coder 交付）
- ❌ 未改动任何 `src/` 源码、`package-lock.json` 或 git 分支状态（`git status` 里只有 Coder 自己的改动）
- ❌ 未安装 Homebrew、未安装任何 `.pkg`

---

## 9. 后续待办（下一步）

1. **等 Coder 交付后**，我跑首次 Rust 全量验证（`cargo fmt --check` → `clippy` → `test`），预计 4–8 分钟，然后出 `cargo test` 真实结果。
2. **`npm run tauri:build` 出 .app/.dmg** 只在本机验证可用性；未签名不能分发，正式发布仍走 tag 触发的 GitHub Actions。
3. **可选清理**：`package-lock.json` 里 41 个 `registry.npmmirror.com` 的 resolved 建议统一回 npmjs，避免换 npm 12 的人再撞 `EALLOWREMOTE`（另开任务，不阻塞本 feature）。
4. **CI 提示**：`.github/workflows/build.yml` 用的是 `node-version: 20` + `npm ci`，与本机 node 24 不同；本机验证通过意味着 Next 16 构建无问题，但如果 CI 出现 node 20 相关的 edge case，可以对齐到 22/24。
