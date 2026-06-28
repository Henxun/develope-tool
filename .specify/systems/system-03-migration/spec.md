---
system: system-03-migration
status: draft
paths:
  - src/app/tools/windows-migrate/
  - src/app/tools/tool-data-migrate/
also_affects:
  - system-02-tauri-backend
---

# Feature Specification: Migration

**System ID**: `system-03-migration`
**Created**: 2026-06-28
**Status**: Draft

## Purpose

The migration features: **Windows program migration** (move an install directory and update
registry entries and environment variables; shortcuts are detection-only) and **tool-data
migration** (relocate directories like `.claude` to another drive with optional
symlink/env-var strategies). This system spans the frontend pages plus the Rust commands that
execute the moves.

## Functional Requirements

- **FR-1**: Validate all migration parameters before mutation — absolute paths only,
  illegal-character rejection, whitelist prefix checks.
- **FR-2**: Default registry writes to **HKCU**; **HKLM** writes require explicit admin
  elevation ("elevate and restart").
- **FR-3**: Use native Rust APIs — never execute PowerShell or shell scripts. Shortcuts are
  detection/notification only.
- **FR-4**: Auto-scan a selected install directory and surface associated registry entries,
  shortcuts, and environment variables.

## Files & Components

- `src/app/tools/windows-migrate/page.tsx` — Windows program migration UI
- `src/app/tools/tool-data-migrate/page.tsx` — tool-data migration UI
- Migration commands in `src-tauri/src/lib.rs` (see `system-02-tauri-backend`)

## Dependencies

- `system-02-tauri-backend` — executes the validated migration operations.

## Success Criteria

- **SC-1**: Every security-sensitive branch (path checks, privilege escalation) has a
  `cargo test` exercising the rejection case.
- **SC-2**: No privileged action runs without passing parameter validation.
