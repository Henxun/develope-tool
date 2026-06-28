# Data Model: 磁盘分析功能优化（参考 SpaceSniffer）

## DiskNode (Rust + TS)

| Field | Type (Rust / TS) | Notes |
|-------|------------------|-------|
| name | String / string | unchanged |
| path | String / string | **NEW** — absolute path (for file actions) |
| size | u64 / number | unchanged |
| nodeType | String / string | "folder" or file category/ext — unchanged |
| children | Vec<DiskNode> / DiskNode[] | unchanged |

## DiskScanResult (Rust + TS) — shape unchanged

| Field | Type | Notes |
|-------|------|-------|
| root | DiskNode | now includes `path` on every node |
| totalSize | u64 / number | unchanged |
| totalFiles | u64 / number | unchanged |
| totalDirs | u64 / number | unchanged |
| scanDurationMs | u64 / number | unchanged |

## FileFilter (TS — frontend view state)

| Field | Type | Notes |
|-------|------|-------|
| categories | Set<string> | empty = all categories |
| minSize | number (bytes) | 0 = no minimum |
| namePattern | string | substring/glob; empty = no name filter |
| hideNonMatching | boolean | false = dim, true = hide |

## FileActionResult (Rust + TS)

| Field | Type | Notes |
|-------|------|-------|
| success | bool / boolean | action outcome |
| message | String / string | error/info detail |

## Behavior notes

- Parallel scan produces identical `DiskNode` tree (modulo `path`) and identical totals vs.
  the sequential scan for the same input.
- Filtering is computed in the frontend; a folder is "matching" if any descendant leaf matches.
- After `delete_path` success, the frontend removes the node from local state and recomputes
  ancestor sizes/totals (no rescan).
