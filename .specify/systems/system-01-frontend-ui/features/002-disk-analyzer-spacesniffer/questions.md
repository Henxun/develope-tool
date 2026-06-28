# Implementation Questions: 磁盘分析功能优化（参考 SpaceSniffer）

**Generated**: 2026-06-28
**Feature**: [spec.md](spec.md)
**Plan**: [plan.md](plan.md)
**Status**: ANSWERED

---

## Q1: Delete mechanism

**Context**: plan.md step 3 / research Decision 3. Delete can use the OS recycle bin or permanent removal.

| Option | Description | Implications |
|--------|-------------|--------------|
| A | `trash` crate → recycle bin | Recoverable, safer; adds a dependency |
| B | Permanent `std::fs::remove_*` + confirm | No new dep; irreversible; riskier |

**Recommended**: A — recoverable deletes are far safer for a disk-cleanup tool; `trash` is well-supported on Windows.

**Answer**: A — trash crate → OS recycle bin.

---

## Q2: Parallel scan crate

**Context**: plan.md step 2 / research Decision 2.

| Option | Description | Implications |
|--------|-------------|--------------|
| A | `rayon` over existing recursive scan | Minimal change; keeps current traversal/sort logic |
| B | `jwalk` (parallel walkdir) | Faster bulk walk; rewrites traversal + size aggregation |

**Recommended**: A — smaller, lower-risk change that preserves the current tree-building and sort behavior while parallelizing.

**Answer**: A — rayon over the existing recursive scan.

---

## Q3: Filter default — dim vs. hide

**Context**: plan.md step 10 / FR-005.

| Option | Description | Implications |
|--------|-------------|--------------|
| A | Dim non-matching (with a "hide" toggle) | Keeps spatial context; SpaceSniffer-like |
| B | Hide non-matching by default | Cleaner focus; loses context |

**Recommended**: A — dimming preserves the treemap's spatial layout (closer to SpaceSniffer), with an optional hard-hide toggle.

**Answer**: A — Dim non-matching by default, with a hide toggle.

---

## Q4: Nested visible depth default

**Context**: plan.md step 8 / FR-002. Nesting must be capped to bound the DOM.

| Option | Description | Implications |
|--------|-------------|--------------|
| A | Default render depth 3, user-adjustable | Good depth perception, bounded DOM |
| B | Default 2 | Lighter DOM; flatter look |
| C | Default 5 | Richest nesting; heavier DOM on big trees |

**Recommended**: A — depth 3 balances the SpaceSniffer nested look against render cost; adjustable for power users.

**Answer**: A — Default nested render depth 3, user-adjustable.

---

## Q5: Delete view update

**Context**: plan.md step 11 / FR-009.

| Option | Description | Implications |
|--------|-------------|--------------|
| A | Local tree mutation + recompute totals | Instant; no rescan; slight bookkeeping |
| B | Trigger a full rescan after delete | Simpler code; slow/jarring on big dirs |

**Recommended**: A — instant feedback without a costly rescan; recompute ancestor sizes locally.

**Answer**: A — Local tree mutation + recompute totals (no rescan).

---

## Q6: Fix pre-existing lint errors in this file

**Context**: plan.md step 12. `disk-heatmap/page.tsx` already has 4 `no-explicit-any` errors + 1 unused var that fail `npm run lint`.

| Option | Description | Implications |
|--------|-------------|--------------|
| A | Fix them as part of this change | File lints clean (SC-006); slightly larger diff |
| B | Leave pre-existing issues | Smaller diff; `npm run lint` still fails on this file |

**Recommended**: A — we're heavily editing this file; leaving known lint failures behind contradicts the constitution's clean-lint goal.

**Answer**: A — Fix the pre-existing lint errors so the file lints clean.

---

## Q7: Build vs. queue

**Context**: Phase 6 decision. This is a larger feature (5 capabilities, frontend + Rust + new deps).

| Option | Description | Implications |
|--------|-------------|--------------|
| A | Build now | Implement immediately in the worktree, then PR |
| B | Queue for later | Commit/push spec, add to Smith queue for batch build |

**Recommended**: A — proceed now; the scope is well-defined.

**Answer**: A — Build now.
