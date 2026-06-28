# Implementation Plan: 工具数据迁移 — 仅软链接策略

**Feature**: `001-symlink-only-migration`
**Spec**: [spec.md](spec.md)
**Branch**: `001-symlink-only-migration`
**Created**: 2026-06-28

## Technical Context

- **Frontend**: Next.js 16 / React 19 / TypeScript — `src/app/tools/tool-data-migrate/page.tsx`
- **Backend**: Rust (Tauri 2) — `src-tauri/src/lib.rs`, command `migrate_tool_data` (Windows impl `migrate_tool_data_windows`)
- **IPC contract**: `ToolDataMigrationRequest` / `ToolDataMigrationResult` (serde `camelCase`)
- **Change type**: simplification + breaking IPC contract change (single local frontend consumer)

## Constitution Gates

- **§Security**: Preserve all path validation, locked-dir check, dry-run, Windows gating. ✅ (only removing the env-var branch, not validation)
- **§Code Quality (Rust)**: No `unwrap`/`panic` on user input; remove resulting dead code so `cargo clippy`/`build` stay clean. ✅
- **§Testing**: Add `cargo test` for the symlink-only request/result shape and that no env var is touched. ✅
- **§UX**: Tailwind, responsive; removing controls only — reuse existing layout. ✅

## Phase 0 — Research

See [research.md](research.md). Key decisions resolved in the questions phase:
- Full removal across UI + Rust (not UI-only).
- Remove env-var-name input + preset `envVarName`.
- Remove env line + `envVarUpdated` from result.

## Phase 1 — Design

- **Data model**: see [data-model.md](data-model.md) — updated request/result/preset shapes.
- **Contract**: see [contracts/migrate_tool_data.md](contracts/migrate_tool_data.md).

## Implementation Steps

### Backend (`src-tauri/src/lib.rs`)

1. **`ToolDataMigrationRequest`** (~L105): remove `strategy: String` and `env_var_name: Option<String>`. Keep `tool_name`, `source_dir`, `target_dir`, `dry_run`.
2. **`ToolDataMigrationResult`** (~L116): remove `strategy: String` and `env_var_updated: bool`. Keep `tool_name`, `source_dir`, `target_dir`, `moved`, `symlink_created`, `warnings`.
3. **`migrate_tool_data_windows`** (~L623):
   - Remove the strategy parse + `matches!` validation (L630–633).
   - The move step (L657–679): drop the `strategy != "env"` guard — always move when not dry-run.
   - Symlink step (L681–698): drop the `strategy == "symlink" || "both"` guard — always run.
   - Remove the entire env-var block (L700–730), including the `set_user_environment_var` call and the `unsafe { env::set_var(...) }`.
   - Update the final `Ok(ToolDataMigrationResult { ... })` to drop `strategy` and `env_var_updated`.
4. **Dead-code cleanup**: after removing the L715 call, check whether `set_user_environment_var` (def ~L1198) is still referenced. `grep` shows its only call site is L715. If `update_windows_environment_vars` does NOT call it internally, it becomes dead → remove it OR mark intent. **Verify by reading L1585–1631**; prefer removal if truly unused to keep `cargo build` warning-free. Do NOT touch `update_windows_environment_vars` / `detect_environment_var_matches` (used by Windows program migration).
5. **Unused imports**: if `std::env` is now unused, remove the import.

### Frontend (`src/app/tools/tool-data-migrate/page.tsx`)

6. **Types**: drop `strategy` from `QuickPreset`; drop `envVarName?`. Drop `strategy`/`envVarUpdated` from `ToolDataMigrationResult`.
7. **`QUICK_PRESETS`**: remove `strategy` and `envVarName` from each entry.
8. **State**: remove `strategy`, `setStrategy`, `envVarName`, `setEnvVarName`.
9. **`executeMigration` / payload**: drop `strategy` and `envVarName` from the payload sent to `invokeTauri("migrate_tool_data", ...)`.
10. **`applyPreset` / `runQuickMigration`**: stop setting strategy/env-var.
11. **Form JSX**: remove the strategy `<select>` (L309–313) and the env-var-name `<input>` (L315).
12. **Result card**: remove the 策略 line (L366) and 环境变量 line (L369).
13. **Copy**: keep helper text accurate; optionally note "采用软链接方式" in the page description.

## Verification

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo build --manifest-path src-tauri/Cargo.toml` (zero warnings ideally)
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `npm run lint`
- Manual (if feasible): `npm run tauri:dev` → verify no strategy/env controls; dry-run shows symlink-only result.

## Risks

- **Dead-code warnings** from removed env path — addressed in step 4/5.
- **Serde shape mismatch** if TS payload still sends removed fields — extra fields are ignored by serde by default, but we remove them anyway for cleanliness (FR-004).
