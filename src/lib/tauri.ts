import { invoke } from "@tauri-apps/api/core";

export async function invokeTauri<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error("当前不是 Tauri 运行环境，请使用 npm run tauri:dev 启动。");
  }

  return invoke<T>(cmd, args);
}

export function parseFilters(raw: string): string[] {
  return raw
    .split(/\r?\n|,/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function toSinglePath(value: string | string[] | null): string | null {
  if (!value) return null;
  return Array.isArray(value) ? value[0] ?? null : value;
}

export function suggestedArchiveExtension(format: string): string {
  if (format === "tar.gz") return "tar.gz";
  if (format === "tar.bz2") return "tar.bz2";
  if (format === "tar.xz") return "tar.xz";
  return format;
}

export function archiveFilterExtension(format: string): string {
  if (format === "tar.gz") return "gz";
  if (format === "tar.bz2") return "bz2";
  if (format === "tar.xz") return "xz";
  return format;
}

export function isWindowsRuntime(): boolean {
  if (typeof window === "undefined") return false;
  const source = `${window.navigator.platform} ${window.navigator.userAgent}`.toLowerCase();
  return source.includes("win");
}
