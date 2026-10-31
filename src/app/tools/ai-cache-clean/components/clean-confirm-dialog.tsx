"use client";

import { useEffect, useRef } from "react";
import type { AiCacheCleanResult } from "@/lib/tauri";
import { formatBytes } from "@/lib/format";
import { CATEGORY_LABELS, humanizeError } from "../utils";

type CleanConfirmDialogProps = {
  preview: AiCacheCleanResult;
  toolNameById: Record<string, string>;
  inUsePaths: string[];
  onCancel: () => void;
  onConfirm: () => void;
};

/**
 * 二次确认弹窗（design.md §2.5）：
 * - 主按钮带体积、rose 危险色；
 * - 初始焦点在「取消」，Esc / 点遮罩 = 取消；
 * - 逐条列出绝对路径（Chief 裁决）。
 */
export default function CleanConfirmDialog({
  preview,
  toolNameById,
  inUsePaths,
  onCancel,
  onConfirm,
}: CleanConfirmDialogProps) {
  const cancelRef = useRef<HTMLButtonElement | null>(null);
  const confirmRef = useRef<HTMLButtonElement | null>(null);

  const wouldDelete = preview.items.filter((item) => item.status === "would_delete");
  const skipped = preview.items.filter(
    (item) => item.status === "skipped" || item.status === "failed",
  );
  const totalBytes = wouldDelete.reduce((sum, item) => sum + item.bytes, 0);
  const toolNames: string[] = [];
  for (const item of wouldDelete) {
    const name = toolNameById[item.toolId] ?? item.toolId;
    if (!toolNames.includes(name)) toolNames.push(name);
  }
  const single = wouldDelete.length === 1 ? wouldDelete[0] : null;
  const title = single
    ? `确认清理「${CATEGORY_LABELS[single.category]}」？`
    : `确认清理 ${wouldDelete.length} 项缓存？`;
  const confirmDisabled = wouldDelete.length === 0;

  useEffect(() => {
    cancelRef.current?.focus();
  }, []);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCancel();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = [cancelRef.current, confirmRef.current].filter(
        (element): element is HTMLButtonElement => element !== null,
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onCancel]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/45 p-4 backdrop-blur-[2px]"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="ai-cache-confirm-title"
        className="flex max-h-[90vh] w-full max-w-[560px] flex-col overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-[0_32px_80px_-24px_rgba(15,23,42,0.45)]"
      >
        <header className="border-b border-slate-200 p-5">
          <div className="flex items-start gap-3">
            <span
              className="mt-0.5 inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-xl border border-rose-200 bg-rose-50 text-rose-600"
              aria-hidden="true"
            >
              <svg
                className="h-5 w-5"
                viewBox="0 0 20 20"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
              >
                <path d="M4 6h12M8 6V4.8c0-.4.4-.8.8-.8h2.4c.4 0 .8.4.8.8V6M6 6l.7 9.2c.1.5.5.8 1 .8h4.6c.5 0 .9-.3 1-.8L14 6" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </span>
            <div className="min-w-0">
              <h2
                id="ai-cache-confirm-title"
                className="text-lg font-semibold text-slate-900"
              >
                {title}
              </h2>
              <p className="mt-1 text-xs text-slate-500">
                {toolNames.length > 0 ? toolNames.join(" · ") : "暂无工具"}
                {wouldDelete.length > 0 ? ` · 预计释放 ${formatBytes(totalBytes)}` : ""}
              </p>
            </div>
          </div>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          <p className="text-sm font-semibold text-slate-900">清理后无法恢复</p>
          <p className="mt-1 text-xs leading-5 text-slate-600">
            这些目录会被
            <strong className="font-semibold text-slate-800">永久删除</strong>
            ，不会进入回收站；AI 工具会在下次使用时自动重建缓存。已保存的对话、配置和项目代码不会被删除。
          </p>

          {wouldDelete.length > 0 ? (
            <ul className="mt-4 space-y-2">
              {wouldDelete.map((item) => (
                <li
                  key={`${item.toolId}-${item.path}`}
                  className="rounded-xl border border-slate-200 bg-slate-50/70 px-3 py-2"
                >
                  <div className="flex items-center justify-between gap-3">
                    <span className="text-xs font-medium text-slate-700">
                      {toolNameById[item.toolId] ?? item.toolId} ·{" "}
                      {CATEGORY_LABELS[item.category]}
                    </span>
                    <span className="font-mono text-xs tabular-nums text-slate-600">
                      {formatBytes(item.bytes)}
                    </span>
                  </div>
                  <p className="mt-1 break-all font-mono text-[11px] leading-4 text-slate-500">
                    {item.path}
                  </p>
                </li>
              ))}
            </ul>
          ) : (
            <p className="mt-4 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800">
              没有可清理的项，目标可能已被删除或已变为符号链接。
            </p>
          )}

          {inUsePaths.length > 0 ? (
            <div className="mt-3 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-xs leading-5 text-amber-800">
              以下 {inUsePaths.length} 项可能正在使用，建议先退出对应 AI 工具：
              <span className="mt-1 block break-all font-mono text-[11px] text-amber-700">
                {inUsePaths.join("、")}
              </span>
            </div>
          ) : null}

          {skipped.length > 0 ? (
            <div className="mt-3 rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-xs leading-5 text-slate-600">
              {skipped.length} 项目标会被跳过：
              {skipped
                .slice(0, 3)
                .map((item) => humanizeError(item.error))
                .join("；")}
            </div>
          ) : null}
        </div>

        <footer className="flex flex-col-reverse gap-2 border-t border-slate-200 px-5 py-4 sm:flex-row sm:items-center sm:justify-between">
          <p className="text-[11px] text-slate-400">清理期间请勿在 AI 工具中执行任务。</p>
          <div className="flex flex-col-reverse gap-2 sm:flex-row">
            <button
              ref={cancelRef}
              type="button"
              onClick={onCancel}
              className="h-10 rounded-xl border border-slate-300 bg-white px-4 text-sm font-medium text-slate-700 transition hover:bg-slate-100 focus-visible:ring-2 focus-visible:ring-[var(--accent)]/40 focus-visible:outline-none"
            >
              取消
            </button>
            <button
              ref={confirmRef}
              type="button"
              onClick={onConfirm}
              disabled={confirmDisabled}
              className="h-10 rounded-xl bg-rose-600 px-4 text-sm font-medium text-white transition hover:bg-rose-700 focus-visible:ring-2 focus-visible:ring-rose-400 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
            >
              {confirmDisabled ? "没有可清理项" : `确认清理 ${formatBytes(totalBytes)}`}
            </button>
          </div>
        </footer>
      </div>
    </div>
  );
}
