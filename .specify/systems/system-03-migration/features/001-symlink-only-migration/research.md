# Research: 工具数据迁移 — 仅软链接策略

## Decision 1 — Removal depth

**Chosen**: Full removal across UI + Rust.
**Alternatives**: UI-only (leaves unreachable Rust env path); default-to-symlink-keep-options
(not truly symlink-only).
**Rationale**: User explicitly chose "Full removal (UI + Rust)". Avoids dead, untested code
paths and yields a clean IPC contract. Acceptable because the only consumer is the local
frontend.

## Decision 2 — Env-var-name input + preset metadata

**Chosen**: Remove the env-var-name input and `envVarName` from presets entirely.
**Rationale**: The field is meaningful only for the removed `env` strategy. Keeping it would
be confusing dead UI.

## Decision 3 — Result payload

**Chosen**: Remove `envVarUpdated` (and `strategy`) from `ToolDataMigrationResult` and the
result card. **Rationale**: Always-false fields are misleading; clean contract preferred over
backward compat (no external consumers).

## Decision 4 — Shared env helpers

**Finding**: `set_user_environment_var` is called only from the tool-data path (lib.rs:715).
`update_windows_environment_vars` and `detect_environment_var_matches` are used by the
**Windows program migration** feature (lib.rs:557, 606) and must remain.
**Action**: After removing the tool-data env block, confirm whether `set_user_environment_var`
becomes unused; if so, remove it (and any now-unused `std::env` import) to keep the build
warning-free. Do not remove the program-migration env helpers.

## Decision 5 — Symlink mechanics

**Finding**: Symlink is created via `create_directory_symlink` →
`std::os::windows::fs::symlink_dir`. No change requested to the mechanism itself.
**Action**: None — reuse as-is; just make it unconditional within tool-data migration.
