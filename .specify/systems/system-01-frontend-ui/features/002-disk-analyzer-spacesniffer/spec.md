---
feature: 002-disk-analyzer-spacesniffer
primary_system: system-01-frontend-ui
also_affects:
  - system-02-tauri-backend
branch: 002-disk-analyzer-spacesniffer
created: 2026-06-28
status: complete
---

# Feature Specification: 磁盘分析功能优化（参考 SpaceSniffer）

**Feature Branch**: `002-disk-analyzer-spacesniffer`
**Created**: 2026-06-28
**Status**: Complete
**Related Issues**: —
**Input**: User description: "优化磁盘分析功能，参考spacesniffer"

## Summary

Upgrade the existing 磁盘分析热力图 (disk-heatmap) tool to deliver a SpaceSniffer-like
experience. Five capabilities are in scope:

1. **Cushion / 3D shading** — replace today's flat colored treemap blocks with SpaceSniffer's
   signature gradient "cushion" rendering for depth perception.
2. **Nested whole-tree view + right-click zoom-out** — render folders-within-folders in a
   single view (not just one level), with double-click to zoom *in* and **right-click to zoom
   *out*** (SpaceSniffer's hallmark navigation), alongside the existing breadcrumb.
3. **Parallel scan engine** — parallelize the Rust directory scan (rayon) so large directories
   scan substantially faster.
4. **Filtering** — a filter control to narrow the view by file type/category, size threshold
   (e.g. `>10MB`), and name pattern; non-matching blocks are dimmed or hidden.
5. **Right-click file actions** — context menu on a block to open the file/folder, show it in
   the system file manager, copy its path, and delete it (with confirmation). Backed by new
   Tauri commands.

Out of scope: live progressive rendering during scan (the scan still completes before the
treemap renders).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Visualize disk usage with cushioned nested treemap (Priority: P1)

A user scans a directory and sees a SpaceSniffer-style treemap: nested folders rendered as
cushioned 3D blocks, sized by disk usage, colored by file category. They can perceive the
folder hierarchy at a glance without drilling in.

**Why this priority**: This is the core visual upgrade and the most visible "looks like
SpaceSniffer" outcome.

**Independent Test**: Scan a folder with nested subfolders; verify blocks show gradient
cushion shading and that nested levels are visible in one view (configurable depth), with
larger items rendered larger.

**Acceptance Scenarios**:

1. **Given** a completed scan, **When** the treemap renders, **Then** blocks display gradient
   cushion shading (not flat fills) and nested child blocks are visible within parent folder
   blocks down to a configurable visible depth.
2. **Given** the treemap, **When** the user double-clicks a folder block, **Then** the view
   zooms into that folder.
3. **Given** a zoomed-in view, **When** the user right-clicks empty treemap space (or uses the
   zoom-out affordance), **Then** the view zooms out one level toward the root.

### User Story 2 - Faster scans on large directories (Priority: P1)

A user scans a large directory tree and the scan completes noticeably faster than the current
single-threaded implementation, while progress is still reported.

**Why this priority**: Scan speed is a primary pain point for a disk analyzer; SpaceSniffer is
known for fast scanning.

**Independent Test**: Scan a directory with many files; the produced tree (sizes, counts) is
identical to the single-threaded result, and wall-clock time is reduced on multi-core machines.

**Acceptance Scenarios**:

1. **Given** a multi-core machine, **When** a large directory is scanned, **Then** results
   (total size, file/dir counts, tree structure) match the previous single-threaded output and
   scanning is parallelized across cores.
2. **Given** a scan in progress, **When** items are processed, **Then** progress events are
   still emitted to the UI.

### User Story 3 - Filter the view (Priority: P2)

A user narrows the treemap to find space hogs: filter by file category (e.g. video), by a
minimum size (e.g. `>100MB`), or by a name pattern (e.g. `*.log`). Non-matching blocks are
dimmed or hidden.

**Why this priority**: Filtering turns visualization into actionable cleanup, a key
SpaceSniffer use case.

**Independent Test**: After a scan, apply each filter type and confirm matching blocks remain
prominent while non-matching are dimmed/hidden, and clearing the filter restores the full view.

**Acceptance Scenarios**:

1. **Given** a scan result, **When** the user sets a size filter `>10MB`, **Then** only files
   ≥10MB (and folders containing them) remain visually prominent.
2. **Given** a scan result, **When** the user enters a name pattern, **Then** matching files
   are highlighted/retained and non-matching are dimmed or hidden.
3. **Given** an active filter, **When** the user clears it, **Then** the full treemap is
   restored.

### User Story 4 - Act on files from the treemap (Priority: P2)

A user right-clicks a block to open the file, reveal it in the system file manager, copy its
full path, or delete it (after confirmation). After deletion the view reflects the change.

**Why this priority**: Lets users clean up space directly, closing the loop SpaceSniffer
provides; but it depends on the visualization existing first.

**Independent Test**: Right-click a file block → menu appears with Open / Reveal / Copy path /
Delete; each action invokes the corresponding backend behavior; delete requires confirmation.

**Acceptance Scenarios**:

1. **Given** a file block, **When** the user chooses "在文件管理器中显示", **Then** the OS file
   manager opens with the item selected.
2. **Given** a file block, **When** the user chooses "复制路径", **Then** the absolute path is
   placed on the clipboard.
3. **Given** a file block, **When** the user chooses "删除" and confirms, **Then** the item is
   sent to the OS recycle bin (preferred) or deleted, and the treemap updates to reflect it.
4. **Given** a delete action, **When** the user cancels the confirmation, **Then** nothing is
   deleted.

### Edge Cases

- **Deep trees**: nested rendering must cap visible depth to stay performant; deeper levels are
  reachable by zooming in.
- **Permission-denied / unreadable dirs**: skipped gracefully as today; parallel scan must not
  panic on errors.
- **Symlinks**: still skipped to avoid cycles.
- **Delete failures** (locked/permission): surface an error; do not crash; treemap unchanged.
- **Delete of a folder**: confirmation must make clear it removes the whole subtree.
- **Filter with no matches**: show an empty/dimmed state with a clear message.
- **Right-click zoom-out at root**: no-op (already at root).
- **Non-Windows**: reveal-in-file-manager and recycle-bin semantics differ per OS; behavior is
  best-effort cross-platform (see Assumptions).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The treemap MUST render blocks with gradient cushion (3D-like) shading instead of
  flat fills, preserving category-based coloring and size-proportional areas.
- **FR-002**: The treemap MUST display nested folder/file hierarchy within a single view down to
  a configurable visible depth, rather than only the immediate children of the current node.
- **FR-003**: Double-click on a folder block MUST zoom in; **right-click on empty treemap area
  MUST zoom out one level** toward the root. The existing breadcrumb navigation MUST remain.
- **FR-004**: The directory scan MUST be parallelized across CPU cores and MUST produce results
  (total size, file/dir counts, tree) equivalent to the current single-threaded scan. Progress
  reporting MUST be preserved.
- **FR-005**: The UI MUST provide filtering by (a) file category/type, (b) minimum size
  threshold, and (c) name pattern (glob or substring). Non-matching blocks MUST be visually
  de-emphasized (dimmed) or hidden, and filters MUST be clearable.
- **FR-006**: A context menu on a block MUST offer: open, show in file manager, copy path, and
  delete. Delete MUST require explicit confirmation.
- **FR-007**: New Tauri backend commands MUST implement: reveal-in-file-manager, and delete
  (preferring OS recycle bin). Open MAY reuse an OS-open mechanism. Copy-path MAY be handled in
  the frontend.
- **FR-008**: All backend file-action commands MUST validate inputs (absolute path, existence)
  and return structured errors; destructive actions MUST be safe (no path traversal, no
  deleting the scan root implicitly without confirmation).
- **FR-009**: After a successful delete, the treemap/state MUST update to reflect the removed
  item without requiring a full rescan (local tree mutation acceptable).

### Key Entities

- **DiskNode** (Rust + TS): name, size, nodeType (category/folder), children, **path** (NEW —
  absolute path, needed for file actions). 
- **DiskScanResult** (Rust + TS): root, totalSize, totalFiles, totalDirs, scanDurationMs
  (unchanged shape; nodes gain `path`).
- **FileFilter** (TS): category set, minSize, namePattern — frontend view state.
- **FileActionRequest/Result** (Rust + TS): path + action outcome for reveal/delete commands.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On a scan of a nested directory, the treemap visibly shows cushioned shading and
  ≥2 levels of nesting in a single view.
- **SC-002**: Right-click zoom-out and double-click zoom-in both work; breadcrumb stays in sync.
- **SC-003**: Parallel scan yields identical totals/structure to the prior scan for the same
  input and completes faster on a multi-core machine (qualitative: not slower; ideally ≥1.5×).
- **SC-004**: Each filter type (category, size, name) correctly narrows the view and is
  clearable.
- **SC-005**: Context-menu actions (open, reveal, copy path, delete-with-confirm) each perform
  their intended effect; delete updates the view.
- **SC-006**: `npm run lint` (touched files), `cargo fmt --check`, `cargo build` (no new
  warnings), and `cargo test` pass.

## Assumptions

- **Cushion shading** is achievable via CSS gradients on the existing custom `TreemapNode`
  (no new charting library); nesting uses the tree we already compute (Nivo nested data or a
  custom nested renderer — decided in plan).
- **Parallel scan** uses the `rayon` crate (add to `src-tauri/Cargo.toml`). Progress counting
  becomes an atomic counter; per-500-items emission is approximated under concurrency.
- **Recycle bin delete** uses a cross-platform crate (e.g. `trash`) if acceptable; otherwise a
  permanent delete with explicit confirmation. Final choice decided in the questions gate.
- **Reveal in file manager**: Windows `explorer /select,`; best-effort on macOS (`open -R`) /
  Linux (`xdg-open` parent). The app's primary target is Windows.
- **Adding `path` to DiskNode** increases payload size; acceptable for the directory sizes this
  tool targets. Visible-depth cap keeps the rendered DOM bounded.
- This optimizes the existing tool in place; no new route/page is added.
