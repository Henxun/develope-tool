---
system: system-02-tauri-backend
status: draft
paths:
  - src-tauri/src/
also_affects: []
---

# Feature Specification: Tauri Backend (Rust)

**System ID**: `system-02-tauri-backend`
**Created**: 2026-06-28
**Status**: Draft

## Purpose

The Rust backend exposed as Tauri 2 IPC commands. It implements all privileged and
filesystem-level operations: system info, directory listing, archive compression/extraction,
and the migration commands. There is no HTTP server — the frontend reaches it only via
`invoke`.

## Files & Components

- `src-tauri/src/lib.rs` — Tauri command implementations
- `src-tauri/tauri.conf.json` — Tauri configuration
- `src-tauri/capabilities/` — scoped Tauri permission capabilities

## Interfaces

Tauri IPC commands (`get_system_info`, `list_directory`, `compress_archive`,
`extract_archive`, migration commands, …) invoked from the frontend.

## Dependencies

- None (leaf system). Consumed by `system-01-frontend-ui`.

## Success Criteria

- **SC-1**: `cargo fmt --check` and `cargo clippy` clean.
- **SC-2**: No `unwrap()`/`panic!` on user-controlled input; errors returned as `Result`.
