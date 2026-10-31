"use client";

import type { AiCacheScanProgressEvent } from "@/lib/tauri";

type ScanProgressBarProps = {
  progress: AiCacheScanProgressEvent | null;
  elapsedMs: number;
};

/**
 * 扫描进度：缓存扫描没有可靠分母，禁止假百分比。
 * 用 spinner + 实时计数 + 已用时表达进度（design.md §2.2）。
 */
export default function ScanProgressBar({ progress, elapsedMs }: ScanProgressBarProps) {
  const currentIndex = progress ? Math.min(progress.toolIndex + 1, progress.toolCount) : 0;
  const countText = progress
    ? `第 ${currentIndex}/${progress.toolCount} 个工具 · 已扫描 ${progress.scannedEntries.toLocaleString("zh-CN")} 项`
    : "正在定位工具目录";
  const elapsedText = elapsedMs > 0 ? ` · 已用时 ${(elapsedMs / 1000).toFixed(1)}s` : "";

  return (
    <div
      className="rounded-2xl border border-sky-200 bg-sky-50 px-4 py-3 text-sm text-sky-800"
      aria-live="polite"
      aria-busy="true"
    >
      <div className="flex flex-wrap items-center gap-3">
        <span
          className="inline-block h-4 w-4 animate-spin rounded-full border-2 border-sky-300 border-t-sky-700"
          aria-hidden="true"
        />
        <p className="font-medium">正在扫描 AI 工具缓存…</p>
        <span className="text-xs text-sky-700">
          {countText}
          {elapsedText}
        </span>
      </div>
      {progress?.currentPath ? (
        <p
          className="mt-2 truncate font-mono text-[11px] text-sky-600"
          title={progress.currentPath}
        >
          {progress.currentPath}
        </p>
      ) : null}
      {elapsedMs > 8000 ? (
        <p className="mt-2 text-xs text-slate-500">缓存目录较大，扫描可能需要更久…</p>
      ) : null}
    </div>
  );
}
