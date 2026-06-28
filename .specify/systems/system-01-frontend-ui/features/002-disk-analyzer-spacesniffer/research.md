# Research: 磁盘分析功能优化（参考 SpaceSniffer）

## Decision 1 — Nested rendering approach

**Options**: (A) feed Nivo deeper nested `children` and let Nivo lay out nested rects;
(B) custom recursive nested renderer.
**Leaning**: A — Nivo `ResponsiveTreeMapHtml` already supports nested data and a custom
`nodeComponent`; we already transform to nested `NivoNode`. Render to a capped visible depth.
**Rationale**: minimal new layout code; reuses the existing custom `TreemapNode`. Final call in
questions gate (nesting depth default).

## Decision 2 — Parallel scan crate

**Chosen**: `rayon`.
**Rationale**: de-facto standard for data parallelism in Rust; `into_par_iter` over directory
entries / `rayon::join` for recursion. Error-tolerant skip preserved. Progress via
`Arc<AtomicU64>`; emission throttled (e.g. every N increments) — approximate under concurrency,
acceptable since progress is cosmetic.
**Alternatives**: std threads + manual pool (more code); `jwalk` (parallel walkdir — heavier,
changes traversal model). Confirm in questions gate.

## Decision 3 — Delete mechanism

**Options**: (A) `trash` crate → OS recycle bin (recoverable, safer); (B) permanent
`std::fs::remove_*` with confirmation.
**Leaning**: A (recycle bin) for safety, with B as fallback if `trash` complicates the Windows
build. **Decided in questions gate.**

## Decision 4 — Reveal in file manager / open

**Chosen**: native commands. Windows: `explorer /select,<path>` (reveal), `ShellExecuteW` or
`explorer <path>` (open) — lib.rs already uses `ShellExecuteW`. mac: `open -R` / `open`. linux:
`xdg-open` on parent / path. Windows is the primary target.

## Decision 5 — Filtering semantics

**Chosen**: frontend-only predicate over the scanned tree. A leaf matches if it passes
category AND size AND name-pattern filters; a folder is retained if any descendant matches.
Non-matching leaves are dimmed (opacity) by default; a "hide non-matching" toggle may hard-hide.
Name pattern: substring or glob (`globset` exists in Rust, but frontend can do simple
glob→regex). **Dim-vs-hide default decided in questions gate.**

## Decision 6 — `path` on DiskNode

**Chosen**: add absolute `path: String` to each node (needed for file actions). Increases
payload; acceptable for targeted directory sizes; visible-depth cap bounds rendered DOM.
