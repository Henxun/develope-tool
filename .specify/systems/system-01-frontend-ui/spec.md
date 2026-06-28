---
system: system-01-frontend-ui
status: draft
paths:
  - src/
also_affects: []
---

# Feature Specification: Frontend UI

**System ID**: `system-01-frontend-ui`
**Created**: 2026-06-28
**Status**: Draft

## Purpose

The Next.js 16 (App Router) + React 19 frontend that renders the desktop toolkit UI inside
the Tauri WebView. It provides tool navigation, page layouts, shared components, and the
typed IPC wrappers used to call Rust commands.

## Files & Components

- `src/app/` — App Router pages and layouts (home, tools shell, individual tool pages)
- `src/components/` — shared UI components (e.g. `recent-tools.tsx`)
- `src/lib/tauri.ts` — typed wrappers around Tauri `invoke`
- `src/lib/tools.ts` — tool registry / navigation metadata

## Interfaces

Calls into the Rust backend exclusively through Tauri IPC via `src/lib/tauri.ts`.

## Dependencies

- `system-02-tauri-backend` — all system operations are delegated to Rust commands.

## Success Criteria

- **SC-1**: ESLint clean (`npm run lint`).
- **SC-2**: UI stays responsive; long operations never block the WebView event loop.
