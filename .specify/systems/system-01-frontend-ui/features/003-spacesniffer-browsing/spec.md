---
feature: 003-spacesniffer-browsing
primary_system: system-01-frontend-ui
also_affects:
  - system-02-tauri-backend
branch: 003-spacesniffer-browsing
created: 2026-06-28
status: in-progress
---

# Feature Specification: SpaceSniffer 浏览观感

**Feature Branch**: `003-spacesniffer-browsing`
**Created**: 2026-06-28
**Input**: User screenshot of SpaceSniffer + "实现spacesniffer这种浏览效果"

## Summary

Refine the disk-heatmap treemap to faithfully reproduce SpaceSniffer's browsing look & feel
shown in the reference screenshot. Six visual/interaction effects:

1. **Centered two-line labels** — file/folder NAME above its SIZE, centered in the block.
2. **Pastel cushion palette** — lighter, SpaceSniffer-like blue/pastel fills with soft cushion.
3. **White selection highlight + green corner triangles** — the hovered/selected block turns
   bright white with small green triangles in its corners.
4. **Parent folder header strip** — a tan header bar above the treemap showing the current
   folder name and total size (e.g. `Footage - 19.8GB`).
5. **Red→green age edges** — thin edge accents on each block encoding file recency
   (red = oldest, green = newest), driven by file modified-time.
6. **Combined filter box** — a single input parsing SpaceSniffer syntax `*.mp4;>500Mb`
   (name globs + size comparisons, `;`-separated), replacing the separate inputs.

## Requirements

- **FR-001**: Each block MUST render a centered two-line label (name + formatted size) when the
  block is large enough; small blocks degrade gracefully (size hidden, then name hidden).
- **FR-002**: Block fills MUST use a lighter pastel cushion palette closer to the screenshot,
  preserving category differentiation but with the SpaceSniffer pastel tone.
- **FR-003**: The hovered/selected block MUST be highlighted bright white with green triangular
  corner markers.
- **FR-004**: A folder header strip MUST appear above the treemap showing the current view's
  folder name and total size.
- **FR-005**: Each block MUST show red→green edge accents mapped from its file modified-time
  across the scanned set (oldest→newest). Folders use their newest descendant's time.
- **FR-006**: The backend `DiskNode` MUST include the file/folder modified time
  (`modifiedSecs`, epoch seconds, 0 if unavailable). Scan results otherwise unchanged.
- **FR-007**: A single combined filter box MUST parse `;`-separated terms: name globs
  (`*.mp4`), and size comparisons (`>500Mb`, `<1gb`) with unit suffixes (b/kb/mb/gb/tb). Terms
  combine with AND. Dim/hide behavior preserved.
- **FR-008**: All existing behavior (nested rendering, double-click zoom in, right-click
  zoom-out, context-menu file actions, parallel scan) MUST be preserved.

## Success Criteria

- **SC-001**: Visual parity with the screenshot: centered labels, pastel cushions, white+green
  selection, folder header, age edges.
- **SC-002**: `*.mp4;>500Mb` narrows the view to mp4 files ≥500MB; clearing restores full view.
- **SC-003**: `cargo fmt --check`, `cargo build` (no new warnings), `cargo test`, `npm run lint`
  (touched file), and `tsc --noEmit` all pass.

## Assumptions

- mtime via `std::fs::Metadata::modified()` → duration since UNIX_EPOCH seconds; 0 on error.
- Age color is computed in the frontend by normalizing each node's `modifiedSecs` between the
  global min/max observed in the scan.
- Folder header replaces/augments the existing breadcrumb (breadcrumb retained for navigation).
- Pastel palette adjusts existing category colors toward lighter tones; legend stays.
