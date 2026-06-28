---
generated: 2026-06-28
project: develope-tool
status: draft
---

# SpecKit Init Intake — develope-tool

This is the permanent record of project configuration decisions. **Do not delete.**
Edit the **Answer** lines and re-run `/smith` to change any decision.

## Codebase Detection Report

| Category | Detected | Confidence | Value |
|----------|----------|------------|-------|
| Project Name | Yes | High | `develope-tool` / "DevToolkit Desktop" |
| Description | Yes | High | Tauri + React + Next.js dev-tools desktop client |
| Structure | Yes | High | Single project (Next.js front + `src-tauri/` Rust backend) |
| Monorepo Tool | No | — | Not a monorepo |
| Frontend | Yes | High | Next.js 16 (App Router) + React 19 + TypeScript |
| Styling | Yes | High | Tailwind CSS 4 (PostCSS) |
| Desktop Container | Yes | High | Tauri 2 |
| Backend | Yes | High | Rust (Tauri commands in `src-tauri/src/`) |
| Database | No | — | Not detected |
| Package Manager | Yes | High | npm + Cargo |
| Auth | No | — | Not detected |
| Linting | Yes | High | ESLint 9 (`eslint-config-next`) |
| Formatting | No | — | Not detected |
| Testing | No | — | Not detected |
| CI/CD | Yes | High | GitHub Actions (`build.yml`, tag-triggered Tauri release) |
| Docker | No | — | Not detected |
| Storybook | No | — | Not detected |
| Default Branch | Yes | High | `main` |

## Group 1 — Project Identity

### Q1. Project name
- **Answer:** develope-tool (display: "DevToolkit Desktop")

### Q2. One-line description
- **Answer:** A cross-platform desktop developer toolkit (Tauri 2 + Next.js 16) providing archive compression/extraction, system info, disk heatmap, Windows program migration, and tool-data migration.

### Q3. Business domain
- **Answer:** Developer productivity / desktop utilities (local filesystem, OS registry & environment operations).

### Q4. Core entities (optional)
- **Answer:** Tool (navigable utility page), ArchiveJob, MigrationPlan (Windows program / tool-data), SystemInfo, DiskUsageNode.

## Group 2 — Tech Stack

### Q5. Frontend framework
- **Answer:** Next.js 16 (App Router) + React 19 — detected, static export consumed by Tauri.

### Q6. Language
- **Answer:** TypeScript (strict) — detected.

### Q7. Styling
- **Answer:** Tailwind CSS 4 via `@tailwindcss/postcss` — detected.

### Q8. Backend framework
- **Answer:** Rust via Tauri 2 commands (`src-tauri/src/lib.rs`) — detected. No HTTP server; IPC commands only.

### Q9. Backend language
- **Answer:** Rust (edition 2021) — detected.

### Q10. API style
- **Answer:** Tauri IPC commands (`invoke`) — not REST/GraphQL.

### Q11. Database
- **Answer:** None — operates on filesystem, Windows registry, environment variables.

### Q12. ORM / data layer
- **Answer:** N/A — no database.

### Q13. Auth
- **Answer:** None (local desktop app). Privilege model: HKCU by default, optional HKLM with admin elevation.

## Group 3 — Development Tooling

### Q14. Package manager (JS)
- **Answer:** npm — detected (`package-lock.json`).

### Q15. Package manager (Rust)
- **Answer:** Cargo — detected.

### Q16. Linting
- **Answer:** ESLint 9 (`eslint-config-next` core-web-vitals + typescript) for JS/TS; `cargo clippy` recommended for Rust.

### Q17. Formatting
- **Answer:** None currently. Recommended: `cargo fmt` for Rust; Prettier optional for TS (not installed — leave out unless desired).

### Q18. Testing framework
- **Answer:** None currently. Recommended: `cargo test` for Rust command logic; Vitest optional for frontend (defer until needed).

### Q19. E2E / Playwright MCP
- **Answer:** No — Tauri desktop app; Playwright web E2E not applicable.

## Group 4 — Quality Standards

### Q20. Test coverage target
- **Answer:** No hard coverage gate initially. Critical Rust command paths (path validation, archive, migration) should have `cargo test` coverage as they grow.

### Q21. Performance requirements
- **Answer:** UI interactions responsive; long operations (compress/extract/migrate/disk-scan) run off the UI thread and report progress. No strict numeric SLA.

### Q22. Accessibility
- **Answer:** Reasonable defaults — keyboard reachable controls, sufficient contrast (per frontend-design skill). No formal WCAG target.

### Q23. Security standards
- **Answer:** Strict input validation on all migration params (absolute paths, illegal-char rejection, whitelist prefixes). Native Rust APIs over shell execution. HKCU-only registry writes by default; HKLM gated behind explicit admin elevation. Tauri capabilities scoped minimally.

## Group 5 — Workflow Preferences

### Q24. Issue tracker
- **Answer:** GitHub Issues (repo: Henxun/develope-tool).

### Q25. Feature-branch naming
- **Answer:** `feat/<short-description>` (matches existing `feat/windows-migration-tool`); also `fix/`, `chore/`.

### Q26. Commit convention
- **Answer:** Conventional Commits in Chinese summary (matches history, e.g. `feat: ...`, `chore: ...`). Include scope where useful.

### Q27. Review gates
- **Answer:** architect (design review) + senior-qa (test/quality review) + staff-infrastructure (CI/Tauri/build review). staff-frontend / staff-backend(Rust) available as needed.

### Q28. Co-author trailer
- **Answer:** Yes — append `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` on Claude-authored commits.

### Q29. Integration / base branch
*(Which branch Smith cuts feature branches from and targets PRs at.)*
- **Answer:** main (detected from `origin/HEAD`).

## Project-Specific Notes

- **Tauri 2 desktop architecture**: Next.js builds a static export (`out/`) that Tauri bundles; there is no running Node server in production. Frontend ↔ backend communication is exclusively Tauri IPC (`@tauri-apps/api` `invoke`). Treat `src-tauri/src/` as the "backend".
- **Platform-conditional code**: Windows-only features (registry/env migration via `winreg`, `windows-sys`) are gated with `#[cfg(windows)]` and only surfaced in the UI on Windows (via `@tauri-apps/plugin-os` detection).
- **Security posture is a first-class concern** — migration operations validate paths and avoid PowerShell; shortcuts are detection-only (no script execution).
- **AGENTS.md note**: This Next.js (v16) has breaking changes vs. training data — read `node_modules/next/dist/docs/` before writing frontend code.
- **CI** is tag-triggered (`v*`) cross-platform Tauri release build; version sourced from `package.json`.
- **Install gap**: This Smith install lacks `commands/` and `smith-index/templates/`; smith subcommands are separate skills (`/smith-new`, `/smith-build`, etc.). `.gitignore`/`.gitattributes` Smith policy merge will be skipped (template absent).
