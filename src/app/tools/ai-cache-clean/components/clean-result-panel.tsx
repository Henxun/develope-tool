"use client";

import type { AiCacheCleanResult } from "@/lib/tauri";
import { formatBytes } from "@/lib/format";
import { humanizeError } from "../utils";

type CleanResultPanelProps = {
  result: AiCacheCleanResult;
  toolNameById: Record<string, string>;
  durationMs: number;
  onRescan: () => void;
  onDismiss: () => void;
};

/** 清理结果：已释放体积 + 成功/失败/跳过明细（design.md §2.7）。 */
export default function CleanResultPanel({
  result,
  toolNameById,
  durationMs,
  onRescan,
  onDismiss,
}: CleanResultPanelProps) {
  const failed = result.items.filter((item) => item.status === "failed");
  const skipped = result.items.filter((item) => item.status === "skipped");
  const touchedTools = new Set(
    result.items.filter((item) => item.status === "deleted").map((item) => item.toolId),
  );
  const tiny = result.deletedCount > 0 && result.freedBytes < 50 * 1024 * 1024;
  const [amount, unit] = formatBytes(result.freedBytes).split(" ");

  return (
    <section
      className="rounded-2xl border border-emerald-200 bg-emerald-50 p-5"
      aria-live="polite"
    >
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-sm font-medium text-emerald-800">
            {tiny ? "缓存已经很干净" : "已释放"}
          </p>
          {tiny ? (
            <p className="mt-1 text-sm font-medium text-emerald-800">
              这次只释放了 {formatBytes(result.freedBytes)}
            </p>
          ) : (
            <p className="mt-1 flex items-baseline gap-2">
              <span className="text-4xl leading-none font-semibold tracking-tight tabular-nums text-emerald-900">
                {amount}
              </span>
              <span className="text-xl font-medium text-emerald-700">{unit}</span>
            </p>
          )}
          <p className="mt-1.5 text-xs text-emerald-700">
            清理 {result.deletedCount} 项
            {touchedTools.size > 0 ? ` · ${touchedTools.size} 个工具` : ""}
            {durationMs > 0 ? ` · 用时 ${(durationMs / 1000).toFixed(1)} s` : ""}
          </p>
        </div>
        <button
          type="button"
          onClick={onDismiss}
          aria-label="关闭清理结果"
          className="inline-flex h-7 w-7 items-center justify-center rounded-lg border border-emerald-200 bg-white/70 text-emerald-700 transition hover:bg-white"
        >
          <svg
            className="h-4 w-4"
            viewBox="0 0 20 20"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            aria-hidden="true"
          >
            <path d="M5 5l10 10M15 5L5 15" strokeLinecap="round" />
          </svg>
        </button>
      </div>

      {failed.length > 0 ? (
        <div className="mt-3 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-xs leading-5 text-amber-800">
          {failed.length} 项未能清理：
          {failed
            .slice(0, 3)
            .map(
              (item) =>
                `${toolNameById[item.toolId] ?? item.toolId}（${humanizeError(item.error)}）`,
            )
            .join("；")}
          {failed.length > 3 ? ` 等 ${failed.length} 项` : ""}。请先退出对应 AI 工具后重试。
        </div>
      ) : null}

      {skipped.length > 0 ? (
        <p className="mt-2 text-xs text-emerald-700">
          {skipped.length} 项已跳过（不存在或为符号链接），未做任何删除。
        </p>
      ) : null}

      <div className="mt-3 flex flex-wrap gap-2">
        <button
          type="button"
          onClick={onRescan}
          className="h-9 rounded-xl bg-[var(--accent)] px-3 text-sm font-medium text-white transition hover:opacity-90 focus-visible:ring-2 focus-visible:ring-[var(--accent)]/40 focus-visible:outline-none"
        >
          重新扫描
        </button>
      </div>
    </section>
  );
}
