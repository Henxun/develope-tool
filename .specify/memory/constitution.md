---
base_branch: main
---

# DevToolkit Desktop (develope-tool) Constitution

Version 1.0.0 · Ratified 2026-06-28

## Core Principles

### I. Code Quality Standards

- **TypeScript/React (frontend):** ESLint (`eslint-config-next` core-web-vitals + typescript)
  MUST pass with zero errors before merge. Components use PascalCase; functions and hooks
  use camelCase. Strict TypeScript — no implicit `any`, no unchecked non-null assertions.
- **Rust (`src-tauri/`):** `cargo fmt` is the canonical formatter; `cargo clippy` SHOULD pass
  without warnings. Tauri commands use snake_case. Errors are returned as `Result<_, String>`
  (or typed errors), never `unwrap()`/`panic!` on user-controlled input.
- **No formatter for TS is mandated** — formatting follows ESLint + editor defaults. Adding
  Prettier later requires a justification and a one-time repo-wide format commit.
- Platform-specific Rust MUST be gated with `#[cfg(windows)]` (etc.); the corresponding UI
  affordance MUST be gated at runtime via `@tauri-apps/plugin-os`.

### II. Testing Standards

- Critical Rust command logic — path validation, archive (compress/extract), and migration
  (Windows program / tool-data) — MUST have `cargo test` coverage, added as that logic grows.
- No hard numeric coverage gate initially. New security-sensitive branches (path checks,
  privilege escalation paths) SHOULD ship with a test exercising the rejection case.
- Frontend automated tests are deferred until a stable component surface exists; manual
  verification via `npm run tauri:dev` is the interim gate for UI changes.

### III. User Experience Consistency

- Apply the **frontend-design** skill (see `AGENTS.md`) to every UI change: defined visual
  theme, purposeful typography, layered backgrounds, restrained motion, mobile-first
  responsiveness within the desktop window, and reuse of existing components/tokens.
- Tailwind utility classes are required; custom CSS MUST be justified.
- Long-running operations (compress, extract, migrate, disk scan) MUST run off the UI thread
  and surface progress/error state — never a frozen window.

### IV. Performance Requirements

- UI interactions remain responsive; heavy work is delegated to Rust commands and reported
  asynchronously. No strict numeric SLA, but no operation may block the WebView event loop.

## Technology Constraints

- **Frontend:** Next.js 16 (App Router, static export) + React 19 + TypeScript 5 + Tailwind CSS 4.
- **Desktop container:** Tauri 2. Production has **no Node server** — the Next.js static export
  in `out/` is bundled by Tauri.
- **Backend:** Rust (edition 2021) Tauri IPC commands in `src-tauri/src/`. Communication with
  the frontend is exclusively via `@tauri-apps/api` `invoke` — no HTTP/REST/GraphQL layer.
- **No database.** State lives in the OS: filesystem, Windows registry, environment variables.
- **Package managers:** npm (JS) and Cargo (Rust) — do not introduce others.
- Read `node_modules/next/dist/docs/` before writing frontend code: this Next.js version has
  breaking changes versus common defaults.

## Security Constraints

- All migration parameters MUST be strictly validated: absolute paths only, illegal-character
  rejection, and whitelist prefix checks — before any filesystem/registry mutation.
- Use native Rust APIs for system operations; do NOT execute PowerShell or shell scripts for
  privileged actions. Shortcut handling is detection-only (no script execution).
- Registry writes default to **HKCU**. **HKLM** writes are opt-in and gated behind explicit
  administrator elevation ("elevate and restart").
- Tauri capabilities (`src-tauri/capabilities/`) MUST be scoped to the minimum required.

## Development Workflow

### Branch Strategy

- Integration / base branch: **`main`** (also recorded in frontmatter `base_branch:`).
- Feature branches are cut from `main` and target `main` in PRs.
- Naming: `feat/<short-description>`, `fix/<short-description>`, `chore/<short-description>`
  (matches existing history, e.g. `feat/windows-migration-tool`).

### SpecKit Workflow

Specify → Clarify → Plan → Tasks → Analyze → Implement, driven by the Smith skills
(`/smith-new`, `/smith-build`, `/smith-implement`, `/smith-bugfix`, `/smith-finish`, etc.).
Artifacts live under `specs/`, `docs/sessions/`, and `.smith/vault/`.

### Commit Standards

- Conventional Commits with a **Chinese summary** (matches history): `feat:`, `fix:`,
  `chore:`, `refactor:`, etc., with an optional scope.
- Claude-authored commits append: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Reference GitHub issues (repo `Henxun/develope-tool`) where applicable.

### Quality Gates

- **architect** — design/architecture review before implementation of non-trivial features.
- **senior-qa** — test and quality review before merge.
- **staff-infrastructure** — review for CI (`.github/workflows/build.yml`), Tauri config, and
  cross-platform build/release concerns.
- `npm run lint` and `cargo fmt --check` MUST be clean before a PR is opened.

## Governance

This constitution governs development of develope-tool. Amendments are made via
`/smith.constitution` (or by editing this file) and bump the version. Conflicts between this
document and ad-hoc requests are resolved in favor of this document unless explicitly amended.

Version 1.0.0 · Ratified 2026-06-28
