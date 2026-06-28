# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

@AGENTS.md

## Project Overview

**DevToolkit Desktop** (`develope-tool`) is a cross-platform desktop developer toolkit built
with **Tauri 2 + Next.js 16**. It bundles several utilities: archive compression/extraction,
system info & directory browsing, a disk-usage heatmap, Windows program migration, and
tool-data migration.

### Project Governance

See `.specify/memory/constitution.md` for binding project principles (code quality, testing,
security, and workflow rules). Read it before non-trivial changes.

### Business Domain

Developer productivity / desktop utilities. Operations target the local **filesystem**,
**Windows registry**, and **environment variables** — there is no database and no web backend.

Core concepts: **Tool** (a navigable utility page), **ArchiveJob**, **MigrationPlan** (Windows
program / tool-data), **SystemInfo**, **DiskUsageNode**.

## Tech Stack

### Frontend
- **Next.js 16** (App Router, **static export** consumed by Tauri) + **React 19**
- **TypeScript 5** (strict)
- **Tailwind CSS 4** via `@tailwindcss/postcss`
- `@nivo/treemap` for the disk heatmap; `@tauri-apps/api` + plugins (`dialog`, `os`) for IPC

> ⚠️ This Next.js version has breaking changes vs. common defaults. **Read
> `node_modules/next/dist/docs/` before writing frontend code** (see `AGENTS.md`).

### Backend (Rust / Tauri)
- **Rust** (edition 2021) Tauri 2 commands in `src-tauri/src/`
- Communication is **exclusively Tauri IPC** (`invoke`) — no HTTP/REST/GraphQL
- Key crates: `zip`, `tar`, `flate2`, `bzip2`, `xz2`, `globset`, `walkdir`; Windows-only
  `winreg` + `windows-sys` (gated with `#[cfg(windows)]`)
- **No database** — state lives in the OS

## Git Commit Guidelines

- **Conventional Commits with a Chinese summary** (matches history): `feat: ...`, `fix: ...`,
  `chore: ...`, etc., with an optional scope.
- Reference GitHub issues (`Henxun/develope-tool`) where applicable.
- Claude-authored commits append: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

## Common Commands

### Frontend / Desktop Development
```bash
npm install          # install JS dependencies
npm run dev          # Next.js dev server (browser only — limited; no Tauri IPC)
npm run tauri:dev    # full desktop dev environment (preferred for UI work)
npm run lint         # ESLint
npm run build        # Next.js static export → out/
npm run tauri:build  # build the desktop bundle
```

### Backend (Rust)
```bash
cargo fmt --manifest-path src-tauri/Cargo.toml            # format
cargo fmt --manifest-path src-tauri/Cargo.toml --check    # CI check
cargo clippy --manifest-path src-tauri/Cargo.toml         # lint
cargo test  --manifest-path src-tauri/Cargo.toml          # tests
```

### CI/CD
- `.github/workflows/build.yml` — tag-triggered (`v*`) cross-platform Tauri release build
  (macOS / Ubuntu 22.04 / Windows). Version is read from `package.json`.

## Architecture

### Backend Structure (`src-tauri/`)
- `src/lib.rs` — Tauri command implementations (`get_system_info`, `list_directory`,
  `compress_archive`, `extract_archive`, migration commands, …)
- `tauri.conf.json` — Tauri configuration
- `capabilities/` — Tauri permission capabilities (keep minimally scoped)

### Frontend Structure (`src/`)
- `app/` — Next.js App Router pages
  - `app/page.tsx` — home
  - `app/tools/layout.tsx` + `app/tools/page.tsx` — tool navigation shell
  - `app/tools/archive/` — compress/extract
  - `app/tools/system/` — system info & directory browser
  - `app/tools/disk-heatmap/` — disk usage treemap
  - `app/tools/windows-migrate/` — Windows program migration
  - `app/tools/tool-data-migrate/` — tool-data (`.claude`, etc.) migration
- `components/` — shared components (e.g. `recent-tools.tsx`)
- `lib/` — `tauri.ts` (IPC wrappers), `tools.ts` (tool registry)

### Key Patterns
- **Tauri IPC boundary**: frontend calls Rust via `invoke`; wrap calls in `src/lib/tauri.ts`.
- **Platform gating**: Windows-only features use `#[cfg(windows)]` in Rust and runtime OS
  detection (`@tauri-apps/plugin-os`) in the UI.
- **Security-first migration**: validate paths (absolute, no illegal chars, whitelist prefix)
  before any mutation; HKCU by default, HKLM only with admin elevation; no PowerShell.

## SpecKit Workflow

This project uses the Smith spec-driven workflow. Artifacts live in `specs/` (feature specs,
plans, tasks, `init-intake.md`), `docs/sessions/` (session chat logs), and `.smith/vault/`
(workflow state + Ledger).

### Smith Skills
| Skill | Purpose |
|-------|---------|
| `/smith-new` | Start a new feature (spec → plan) |
| `/smith-build` | Autonomous build: tasks → implement → test → PR |
| `/smith-implement` | Execute the task breakdown |
| `/smith-bugfix` | Lightweight fix workflow |
| `/smith-finish` | Finalize / merge / release notes |
| `/smith-analyze` | Cross-artifact consistency check |
| `/smith-checklist` | Generate a quality checklist |
| `/smith-explore`, `/smith-bank`, `/smith-vault` | Explore, capture ideas, manage vault |

### Review Gates (Mandatory)
- **architect** — design review before implementing non-trivial features
- **senior-qa** — test & quality review before merge
- **staff-infrastructure** — CI / Tauri config / cross-platform build review

Agent definitions live in `.claude/agents/`.

### Branch Naming
`feat/<desc>`, `fix/<desc>`, `chore/<desc>`. Base/integration branch: **`main`**.

### When Asked "What's Next?"
Check the active workflow under `.smith/vault/active-workflows/`, the current `specs/` feature,
and open tasks, then propose the next Smith step.
