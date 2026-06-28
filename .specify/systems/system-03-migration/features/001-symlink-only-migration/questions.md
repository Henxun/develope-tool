# Implementation Questions: 工具数据迁移 — 仅软链接策略

**Generated**: 2026-06-28
**Feature**: [spec.md](spec.md)
**Plan**: [plan.md](plan.md)
**Status**: ANSWERED

---

## Q1: Unused `set_user_environment_var` after removing the env block

**Context**: plan.md step 4. After removing the tool-data env-var block, `set_user_environment_var` (lib.rs:1198) loses its only call site (lib.rs:715). The two other env helpers stay (used by Windows program migration).

**Question**: How to handle the now-unused function?

| Option | Description | Implications |
|--------|-------------|--------------|
| A | Remove it (and any newly-unused `std::env` import) | Cleanest; zero `dead_code` warnings; smaller surface |
| B | Keep it, annotate `#[allow(dead_code)]` | Preserves helper for future use; mild clutter |
| C | Keep it untouched | Risks `cargo build` dead-code warning, violates clean-build goal |

**Recommended**: A — keeps `cargo build` warning-free per constitution §Code Quality. Git history preserves it if needed later.

**Answer**: A — Remove set_user_environment_var and any now-unused std::env import.

---

## Q2: Add a Rust unit test for the symlink-only flow?

**Context**: spec.md US2 / SC-004; constitution §Testing wants `cargo test` for critical migration logic. The project currently has no tests.

**Question**: Should we add test coverage as part of this change?

| Option | Description | Implications |
|--------|-------------|--------------|
| A | Add a focused dry-run test asserting result shape has no env fields + symlink path is taken | Satisfies §Testing; small effort; first test in repo |
| B | Skip tests for now | Faster; leaves SC-004 verified only manually |

**Recommended**: A — small, establishes the testing baseline the constitution asks for. Dry-run test avoids needing real filesystem mutation.

**Answer**: A — Add a focused dry-run Rust test for the symlink-only flow.

---

## Q3: Page description copy

**Context**: page.tsx:271 description mentions presets. With symlink-only, we may clarify the method.

**Question**: Update the page description to state symlink method?

| Option | Description | Implications |
|--------|-------------|--------------|
| A | Add "采用软链接（symlink）方式迁移" to the description | Clearer to users; tiny copy change |
| B | Leave description as-is | No change; method implicit |

**Recommended**: A — sets correct expectation now that there's no strategy choice.

**Answer**: A — Add 采用软链接（symlink）方式 to the page description.

---

## Q4: Build vs. queue

**Context**: Phase 6 decision.

**Question**: Build now or queue for later?

| Option | Description | Implications |
|--------|-------------|--------------|
| A | Build now | Implement immediately in the worktree, then PR |
| B | Queue for later | Commit/push spec, add to Smith queue |

**Recommended**: A — small, well-scoped change; build now.

**Answer**: A — Build now.
