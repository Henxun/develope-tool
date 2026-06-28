# IPC Contracts: 磁盘分析功能优化

## `scan_disk_usage` (existing — node gains `path`)

**Request**: `{ rootPath: string, maxDepth?: number, includeHidden?: boolean }`
**Response**: `DiskScanResult` — every `DiskNode` now carries `path` (absolute).
**Behavior change**: scan is parallelized internally; results equivalent to sequential.

## `open_path` (NEW)

**Request**: `{ path: string }`
**Response**: `FileActionResult { success, message }`
**Behavior**: open file/folder with OS default handler. Windows `ShellExecuteW`/`explorer`.

## `reveal_in_file_manager` (NEW)

**Request**: `{ path: string }`
**Response**: `FileActionResult`
**Behavior**: open OS file manager with item selected. Windows: `explorer /select,<path>`.
mac `open -R <path>`; linux `xdg-open <parent>`.

## `delete_path` (NEW)

**Request**: `{ path: string }`
**Response**: `FileActionResult`
**Behavior**: validate absolute + exists + not a drive root; send to recycle bin (preferred) or
delete. Confirmation is enforced in the UI before calling.
**Errors**: `路径不能为空` / `路径必须是绝对路径` / `路径不存在` / `禁止删除驱动器根目录` / OS error.

## Validation (all new commands)

- Trim input; reject empty.
- Require absolute path.
- Require existence (open/reveal/delete).
- `delete_path`: additionally reject drive/filesystem roots; no path-traversal beyond the given
  absolute path.
