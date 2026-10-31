/** 紧凑的字节格式化：缓存清理页与后续工具共用。 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const base = 1024;
  const index = Math.min(
    Math.floor(Math.log(bytes) / Math.log(base)),
    units.length - 1,
  );
  const value = bytes / Math.pow(base, index);
  const digits = index === 0 ? 0 : value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[index]}`;
}

export function formatNumber(value: number): string {
  return value.toLocaleString("zh-CN");
}

export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return "0s";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

/** 扫描完成时间（HH:mm:ss）。 */
export function formatClock(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString("zh-CN", { hour12: false });
}
