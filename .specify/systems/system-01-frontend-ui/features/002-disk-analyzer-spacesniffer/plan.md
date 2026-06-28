# Implementation Plan: 磁盘分析功能优化（参考 SpaceSniffer）

**Feature**: `002-disk-analyzer-spacesniffer`
**Spec**: [spec.md](spec.md)
**Branch**: `002-disk-analyzer-spacesniffer`
**Created**: 2026-06-28

## Technical Context

- **Frontend**: `src/app/tools/disk-heatmap/page.tsx` (Nivo `ResponsiveTreeMapHtml` + custom `TreemapNode`)
- **Backend**: `src-tauri/src/lib.rs` — `scan_disk_usage`, `scan_directory_recursive`, `DiskNode`, `DiskScanResult`
- **Existing deps**: `globset` (name patterns), `tauri-plugin-dialog`, `tauri-plugin-os`. No opener/shell plugin.
- **New deps (proposed)**: `rayon` (parallel scan); `trash` (recycle-bin delete) — confirmed in questions gate.

## Constitution Gates

- **§Security**: file-action commands validate absolute path + existence, no traversal; delete confirmed in UI and prefers recycle bin. ✅
- **§Code Quality (Rust)**: no `unwrap`/`panic` on user input; parallel scan tolerates errors; keep build warning-free. ✅
- **§Testing**: `cargo test` for (a) parallel-scan equivalence to sequential on a temp tree, (b) file-action path validation rejection. ✅
- **§UX (frontend-design)**: cushion shading via CSS gradients, restrained transitions, responsive; reuse existing layout/tokens. ✅

## Phase 0 — Research

See [research.md](research.md). Key decisions (most finalized in the questions gate):
- Nested rendering approach (Nivo nested data vs. custom nested renderer).
- Parallel scan crate (`rayon`) + progress under concurrency (atomic counter).
- Delete mechanism (`trash` crate vs. permanent delete).
- Reveal-in-file-manager per OS.

## Phase 1 — Design

- **Data model**: [data-model.md](data-model.md) — `DiskNode` gains `path`; new `FileActionResult`; frontend `FileFilter`.
- **Contracts**: [contracts/](contracts/) — `scan_disk_usage` (unchanged signature, node gains path), `reveal_in_file_manager`, `delete_path`, `open_path`.

## Implementation Steps

### Backend (`src-tauri/src/lib.rs`, `Cargo.toml`)

1. **Add `path: String` to `DiskNode`** (absolute path). Populate in `scan_directory_recursive` for both dir and file nodes. Update `count_tree_items` (no change needed) and any constructors.
2. **Parallelize scan** with `rayon`:
   - Add `rayon = "1"` to `Cargo.toml`.
   - In `scan_directory_recursive`, collect dir entries, then process subdirectories with `par_iter`/`into_par_iter` (e.g. `rayon::join` or a parallel map), summing sizes. Keep error-tolerance (skip unreadable).
   - Replace `items_scanned: &mut u64` with an `Arc<AtomicU64>` so concurrent threads can increment; emit progress periodically (best-effort under concurrency — emit from a throttled point).
   - Preserve sort-by-size-desc and the dir-then-file ordering after parallel collection.
3. **New commands**:
   - `open_path(path)` — open a file/folder with the OS default handler (Windows `ShellExecuteW`/`explorer`; mac `open`; linux `xdg-open`). Reuse existing Windows `ShellExecuteW` usage pattern in lib.rs.
   - `reveal_in_file_manager(path)` — Windows `explorer /select,<path>`; mac `open -R`; linux open parent via `xdg-open`.
   - `delete_path(path)` — validate absolute + exists + not a root/drive; prefer `trash` crate (recycle bin); return `FileActionResult { success, message }`.
   - All validate input and return `Result<_, String>` or a structured result.
4. **Register** new commands in `tauri::generate_handler!`.
5. **Tests** (`#[cfg(test)] mod tests`): parallel vs. sequential equivalence on a `tempfile` tree; `delete_path` rejects empty/relative paths.

### Frontend (`src/app/tools/disk-heatmap/page.tsx`)

6. **Types**: add `path` to `DiskNode` and `NivoNode`; add `FileFilter` state type; add `FileActionResult` type.
7. **Cushion shading**: update `TreemapNode` styling to use a radial/linear CSS gradient (light top-left → darker bottom-right) over the category color, plus subtle inner border/inset shadow for the cushion effect. Keep label rendering.
8. **Nested view**: render nested children to a configurable visible depth instead of flattening one level. Either feed Nivo nested data with deeper `children` or render a recursive nested layout. Add a "visible depth" control (reuse/relabel maxDepth or add a render-depth slider). Cap to bound DOM.
9. **Zoom-out on right-click**: add `onContextMenu` to the treemap container → `preventDefault()` + pop one level off `zoomPath` (no-op at root). Keep double-click zoom-in and breadcrumb.
10. **Filtering UI**: add a filter bar — category multiselect (reuse `CATEGORY_LABELS`), min-size input (with unit), name pattern input. Compute a predicate; dim (reduce opacity) or hide non-matching leaf blocks; keep ancestor folders if any descendant matches. Add a clear button.
11. **Context menu**: on block right-click (file nodes; distinguish from container zoom-out), show a small menu: 打开 / 在文件管理器中显示 / 复制路径 / 删除. Wire to `invokeTauri` calls; copy-path uses the clipboard. Delete → confirm dialog → `delete_path` → on success, remove node from local tree state and recompute totals (no full rescan).
12. **Lint cleanliness**: this file currently has pre-existing `no-explicit-any` errors (152, 228, 323, 555) and an unused `breadcrumbItems` (306). Since we're substantially editing it, fix the `any`s we touch and remove/也使用 `breadcrumbItems`, so the file lints clean after the change (supports SC-006 for touched files).

## Verification

- `cargo fmt --check`, `cargo build` (no new warnings), `cargo test`
- `npm run lint` on `disk-heatmap/page.tsx` → clean
- Manual (`npm run tauri:dev`): scan a nested folder → cushioned nested treemap; double-click in / right-click out; apply each filter; right-click file → actions; delete with confirm updates view.

## Risks

- **Parallel progress accuracy**: progress under concurrency is approximate — acceptable (cosmetic).
- **DOM size with nesting**: mitigated by visible-depth cap.
- **`trash` crate availability/build on Windows**: fallback to permanent delete with confirmation if it complicates the build (decided in questions).
- **Right-click overload**: empty-space right-click = zoom out vs. block right-click = context menu — must be disambiguated by event target.
