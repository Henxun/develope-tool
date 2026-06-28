---
feature: 001-symlink-only-migration
primary_system: system-03-migration
also_affects:
  - system-02-tauri-backend
branch: 001-symlink-only-migration
created: 2026-06-28
status: in-progress
---

# Feature Specification: 工具数据迁移 — 仅软链接策略

**Feature Branch**: `001-symlink-only-migration`
**Created**: 2026-06-28
**Status**: In Progress
**Related Issues**: —
**Input**: User description: "优化工具数据迁移功能，仅支持软连接方式"

## Summary

The 工具数据迁移 (tool-data migration) tool currently supports three strategies:
`symlink`, `env` (set an environment variable to the new location), and `both`. This feature
simplifies the tool to a **single, symlink-only** strategy across the entire stack — frontend
and Rust backend. The environment-variable code paths, the strategy selector, the env-var-name
input, preset `envVarName` values, and the env-var result line are all removed. The resulting
flow is unambiguous: move the directory to the target, then create a directory symlink at the
original location so existing tools continue to resolve the old path transparently.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Migrate a tool-data directory via symlink (Priority: P1)

A Windows user wants to move a large tool-data directory (e.g. `.rustup`, `.nuget`, `.claude`)
off the system drive to free space, without breaking the tool that uses it. They pick a source
directory and a target location and run the migration. The tool moves the data and leaves a
directory symlink behind at the original path. There is no strategy to choose — symlink is the
only behavior.

**Why this priority**: This is the entire purpose of the feature — a single, foolproof
migration action.

**Independent Test**: In dry-run, selecting a preset and running shows the planned move +
symlink with no strategy/env-var UI present. With dry-run off (Windows), the data is moved and
a working symlink is created at the source path; the source path still resolves to the data.

**Acceptance Scenarios**:

1. **Given** the tool-data migration page on Windows, **When** the page renders, **Then** no
   strategy selector and no environment-variable-name input are shown.
2. **Given** a valid source and target and dry-run enabled, **When** the user runs migration,
   **Then** the result reports the planned source → target move and symlink, and reports no
   environment-variable activity.
3. **Given** a valid source and target and dry-run disabled, **When** the user runs migration,
   **Then** the directory is moved to the target and a directory symlink is created at the
   source path, and the result shows `符链接: 已创建` and no env-var line.
4. **Given** a quick-migrate preset, **When** the user clicks it, **Then** the migration runs
   with the symlink strategy regardless of any legacy preset metadata.

### User Story 2 - Backend rejects/ignores non-symlink strategy input (Priority: P2)

The Rust `migrate_tool_data` command no longer performs env-var work. Any request is treated as
symlink-only; the command shape no longer requires a strategy or env-var-name.

**Why this priority**: Guarantees the simplification is real (no UI-unreachable env code path
left behind) and the command contract is clean.

**Independent Test**: `cargo test` covers that the request struct no longer needs `strategy`
/`env_var_name`, and that the result no longer carries `env_var_updated`. A request always
yields a symlink-only result.

**Acceptance Scenarios**:

1. **Given** a migration request, **When** the backend executes it, **Then** it performs only
   the move + symlink steps and never reads or writes any environment variable.
2. **Given** the `ToolDataMigrationResult`, **When** it is serialized to the frontend, **Then**
   it contains no `envVarUpdated` field.

### Edge Cases

- **Target exists and non-empty**: unchanged — reject with the existing "目标目录已存在且非空"
  error.
- **Source == target / nested paths**: unchanged — reject as today.
- **Symlink creation fails** (permission / already exists): surface as a warning, consistent
  with current behavior; do not silently succeed.
- **Source directory locked** (`check_directory_locked`): unchanged — block real (non-dry-run)
  migration with the existing locked-files error.
- **dry-run**: no move, no symlink creation; result clearly indicates simulation.
- **Non-Windows**: tool remains Windows-only (unchanged gating).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The tool-data migration MUST use the symlink strategy exclusively. The `env` and
  `both` strategies MUST be removed from the UI and the Rust backend.
- **FR-002**: The UI MUST NOT present a strategy selector or an environment-variable-name input.
- **FR-003**: Quick-migrate presets MUST NOT carry or rely on `strategy` or `envVarName`; all
  presets perform a symlink migration.
- **FR-004**: The Rust `migrate_tool_data` command contract MUST drop the `strategy` and
  `env_var_name` request fields and the `env_var_updated` / `strategy` result fields (or pin
  them such that no env-var code is reachable — full removal is preferred).
- **FR-005**: The Rust backend MUST contain no reachable environment-variable read/write code as
  part of tool-data migration (`set_user_environment_var` calls within the tool-data path
  removed). Shared helpers used elsewhere (Windows program migration) MUST be left intact.
- **FR-006**: The result card MUST NOT display an environment-variable line; it shows tool,
  source → target, moved, and symlink-created status (plus warnings).
- **FR-007**: All existing safety validations (path normalization, source/target equality &
  nesting checks, locked-directory check, dry-run behavior, Windows-only gating) MUST be
  preserved.

### Key Entities

- **ToolDataMigrationRequest** (Rust + TS): tool name, source dir, target dir, dry-run. No
  longer includes strategy or env-var name.
- **ToolDataMigrationResult** (Rust + TS): tool name, source/target dir, moved, symlinkCreated,
  warnings. No longer includes strategy or envVarUpdated.
- **QuickPreset** (TS): label, toolName, folderName. No longer includes strategy or envVarName.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The tool-data migration UI shows zero strategy/env-var controls; a user can
  complete a migration with only source + target inputs.
- **SC-002**: `grep` for env-var migration symbols in the tool-data path (`env_var_name`,
  `env_var_updated`, `CLAUDE_CONFIG_DIR` defaulting, env `set_var` within `migrate_tool_data`)
  returns no reachable matches in the tool-data migration flow.
- **SC-003**: `npm run lint`, `cargo fmt --check`, and `cargo build` all pass on the branch.
- **SC-004**: A migration request (dry-run and real) produces a symlink-only result with no
  env-var fields present in the payload.

## Assumptions

- "软连接方式" (symlink) refers to the existing Windows directory symlink created via
  `create_directory_symlink` (`std::os::windows::fs::symlink_dir`). No change to symlink
  mechanics is requested — only the removal of the alternative strategies.
- This is an acceptable breaking change to the `migrate_tool_data` IPC contract; there are no
  external API consumers (local desktop app, single frontend caller).
- The shared environment-variable helpers (`set_user_environment_var`,
  `update_windows_environment_vars`, etc.) remain in use by the **Windows program migration**
  feature and MUST NOT be deleted — only their use within the tool-data path is removed.
