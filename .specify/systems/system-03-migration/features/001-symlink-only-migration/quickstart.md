# Quickstart: 工具数据迁移 — 仅软链接策略

## Manual verification (Windows)

```bash
npm install
npm run tauri:dev
```

1. Navigate to **工具数据迁移**.
2. Confirm the page shows: target-root + presets, tool name, source dir, target dir, dry-run
   checkbox, submit. **No** strategy `<select>`, **no** environment-variable-name input.
3. With dry-run ON, click a preset (e.g. `.rustup`) → result card shows tool, source → target,
   移动目录 (no), 软链接 (未创建, dry-run), warnings. **No** 策略 line, **no** 环境变量 line.
4. (Optional, real run) With dry-run OFF and a safe test directory: run → data moved, symlink
   created at source, result shows 软链接: 已创建.

## Build / lint checks

```bash
npm run lint
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo build --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```
