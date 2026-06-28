# Data Model: 工具数据迁移 — 仅软链接策略

## ToolDataMigrationRequest

| Field | Type (Rust / TS) | Notes |
|-------|------------------|-------|
| toolName | String / string | unchanged |
| sourceDir | String / string | unchanged |
| targetDir | String / string | unchanged |
| dryRun | bool / boolean | unchanged |
| ~~strategy~~ | — | **removed** |
| ~~envVarName~~ | — | **removed** |

## ToolDataMigrationResult

| Field | Type (Rust / TS) | Notes |
|-------|------------------|-------|
| toolName | String / string | unchanged |
| sourceDir | String / string | unchanged |
| targetDir | String / string | unchanged |
| moved | bool / boolean | unchanged |
| symlinkCreated | bool / boolean | unchanged |
| warnings | Vec<String> / string[] | unchanged |
| ~~strategy~~ | — | **removed** |
| ~~envVarUpdated~~ | — | **removed** |

## QuickPreset (TS only)

| Field | Type | Notes |
|-------|------|-------|
| label | string | unchanged |
| toolName | string | unchanged |
| folderName | string | unchanged |
| ~~strategy~~ | — | **removed** |
| ~~envVarName~~ | — | **removed** |

## Behavior

Single strategy: **move directory → create directory symlink at source path**. Always applied
when not dry-run. No environment-variable interaction in this flow.
