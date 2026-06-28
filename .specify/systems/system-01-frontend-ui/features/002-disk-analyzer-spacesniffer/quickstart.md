# Quickstart: 磁盘分析功能优化（参考 SpaceSniffer）

## Manual verification (Windows)

```bash
npm install
npm run tauri:dev
```

1. Open **磁盘分析热力图**, scan a nested folder (e.g. a project dir).
2. **Cushion + nested**: blocks show 3D gradient shading; ≥2 nesting levels visible at once.
3. **Navigation**: double-click a folder → zooms in; right-click empty treemap area → zooms out
   one level; breadcrumb stays in sync.
4. **Speed**: large folder scans complete faster than before; progress still updates.
5. **Filter**: set size `>10MB`, pick a category, type a name pattern → non-matching blocks dim
   (or hide); clear → full view restored.
6. **File actions**: right-click a file block → 打开 / 在文件管理器中显示 / 复制路径 / 删除.
   Delete asks for confirmation; on confirm the block disappears and totals update.

## Build / lint checks

```bash
npm run lint                                              # touched file clean
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo build --manifest-path src-tauri/Cargo.toml          # no new warnings
cargo test  --manifest-path src-tauri/Cargo.toml          # parallel-equiv + validation
```
