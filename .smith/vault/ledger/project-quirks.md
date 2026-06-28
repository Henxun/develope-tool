# Project Quirks

Project-specific oddities and constraints. Populated by `/smith-reflect`.

- **Tauri 2 desktop app**: Next.js static export bundled by Tauri; no production Node server. Frontend↔backend via Tauri IPC (`invoke`) only.
- **Next.js 16 breaking changes**: Read `node_modules/next/dist/docs/` before writing frontend code (per AGENTS.md).
- **Windows-only code** gated with `#[cfg(windows)]`; UI gates Windows features via `@tauri-apps/plugin-os`.
- **Security-critical**: migration param validation (absolute paths, illegal chars, whitelist prefixes); native Rust APIs, no PowerShell.
