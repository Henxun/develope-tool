# IPC Contract: `migrate_tool_data`

**Command**: `migrate_tool_data` (Tauri IPC, Windows-only effective behavior)
**Caller**: `src/app/tools/tool-data-migrate/page.tsx` via `invokeTauri("migrate_tool_data", { request })`

## Request (after change)

```jsonc
{
  "request": {
    "toolName": "rustup",
    "sourceDir": "%USERPROFILE%\\.rustup",
    "targetDir": "D:\\tool-data\\.rustup",
    "dryRun": true
  }
}
```

- No `strategy` field.
- No `envVarName` field.

## Response (after change)

```jsonc
{
  "toolName": "rustup",
  "sourceDir": "C:\\Users\\me\\.rustup",
  "targetDir": "D:\\tool-data\\.rustup",
  "moved": false,
  "symlinkCreated": false,
  "warnings": []
}
```

- No `strategy` field.
- No `envVarUpdated` field.

## Errors (unchanged)

- `工具数据迁移仅支持 Windows 平台` (non-Windows)
- `源目录和目标目录不能相同` / `...不能互相包含`
- `目标目录已存在且非空，请更换目标目录`
- locked-directory error (real run only)

## Behavior contract

1. Validate + normalize source/target (unchanged).
2. If not dry-run: move directory to target (copy+remove or move-with-fallback).
3. If not dry-run: create directory symlink at source path → target.
4. Never read/write environment variables.
