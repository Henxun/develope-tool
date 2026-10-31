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



// ── AI 智能体缓存清理（/tools/ai-cache-clean） ──

export type AiCacheCategory = "cache" | "logs" | "sessions" | "model_cache";

export type AiRisk = "safe" | "history" | "redownload";

export type AiPathKind = "dir" | "file";

export type AiCleanStatus = "would_delete" | "deleted" | "skipped" | "failed";

export type AiCachePath = {
  path: string;
  bytes: number;
  kind: AiPathKind;
  lastModifiedMs: number;
  inUse: boolean;
};

export type AiCacheCategoryInfo = {
  category: AiCacheCategory;
  label: string;
  risk: AiRisk;
  /** false = 仅展示，不提供任何删除入口（sessions / model_cache）。 */
  cleanable: boolean;
  defaultSelected: boolean;
  description: string;
  paths: AiCachePath[];
  bytes: number;
};

export type AiCacheRoot = {
  path: string;
  exists: boolean;
  categories: AiCacheCategoryInfo[];
  warnings: string[];
};

export type AiToolScan = {
  id: string;
  name: string;
  vendor: string;
  detected: boolean;
  roots: AiCacheRoot[];
  cleanableBytes: number;
  displayOnlyBytes: number;
  warnings: string[];
};

export type AiCacheScanResult = {
  platform: string;
  catalogVersion: number;
  scannedAtSecs: number;
  durationMs: number;
  totalCleanableBytes: number;
  totalDisplayOnlyBytes: number;
  tools: AiToolScan[];
  warnings: string[];
};

export type AiToolCatalog = {
  id: string;
  name: string;
  vendor: string;
  roots: string[];
  detected: boolean;
  protected: string[];
};

export type AiCacheCatalog = {
  platform: string;
  catalogVersion: number;
  tools: AiToolCatalog[];
};

export type AiCacheCleanTarget = {
  toolId: string;
  category: AiCacheCategory;
  /** 扫描返回的绝对路径；后端会重新做白名单校验。 */
  path: string;
};

export type AiCleanItem = {
  toolId: string;
  category: AiCacheCategory;
  path: string;
  bytes: number;
  status: AiCleanStatus;
  error: string | null;
};

export type AiCacheCleanResult = {
  dryRun: boolean;
  freedBytes: number;
  deletedCount: number;
  skippedCount: number;
  failedCount: number;
  items: AiCleanItem[];
  warnings: string[];
};

export type AiCacheScanProgressEvent = {
  toolId: string;
  currentPath: string;
  scannedEntries: number;
  toolIndex: number;
  toolCount: number;
};

export type AiCacheCleanProgressEvent = {
  toolId: string;
  path: string;
  deletedCount: number;
  freedBytes: number;
};

export async function getAiCacheCatalog(): Promise<AiCacheCatalog> {
  return invokeTauri<AiCacheCatalog>("get_ai_cache_catalog");
}

export async function scanAiCacheUsage(toolIds: string[] = []): Promise<AiCacheScanResult> {
  return invokeTauri<AiCacheScanResult>("scan_ai_cache_usage", {
    request: { toolIds },
  });
}

export async function cleanAiCaches(request: {
  targets: AiCacheCleanTarget[];
  dryRun?: boolean;
}): Promise<AiCacheCleanResult> {
  return invokeTauri<AiCacheCleanResult>("clean_ai_caches", { request });
}
