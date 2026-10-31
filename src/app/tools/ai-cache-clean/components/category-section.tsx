"use client";

import { useEffect, useRef, useState } from "react";
import type { AiCacheCategoryInfo, AiRisk } from "@/lib/tauri";
import { formatBytes } from "@/lib/format";

const RISK_STYLES: Record<AiRisk, { label: string; className: string }> = {
  safe: {
    label: "安全",
    className: "border-emerald-200 bg-emerald-50 text-emerald-700",
  },
  history: {
    label: "谨慎",
    className: "border-amber-200 bg-amber-50 text-amber-800",
  },
  redownload: {
    label: "需重新下载",
    className: "border-rose-300 bg-rose-50 text-rose-700",
  },
};

type CategorySectionProps = {
  category: AiCacheCategoryInfo;
  selected: Set<string>;
  busy: boolean;
  onToggleCategory: (paths: string[], selectAll: boolean) => void;
  onTogglePath: (path: string) => void;
  onOpenPath: (path: string) => void;
  /** 仅展示类别：点击后按需计算体积（Chief 裁决 2，includeHeavy 二次请求）。 */
  onMeasureHeavy?: () => void;
  heavyLoading?: boolean;
  /** 后台扫描未结束时禁用「计算占用」，避免并发扫描。 */
  heavyBlocked?: boolean;
};

/** 单个类别行：类别勾选 + 风险徽标 + 体积 + 展开后的路径明细。 */
export default function CategorySection({
  category,
  selected,
  busy,
  onToggleCategory,
  onTogglePath,
  onOpenPath,
  onMeasureHeavy,
  heavyLoading = false,
  heavyBlocked = false,
}: CategorySectionProps) {
  const [expanded, setExpanded] = useState(false);
  const checkboxRef = useRef<HTMLInputElement | null>(null);

  const paths = category.paths;
  const selectedCount = paths.filter((path) => selected.has(path.path)).length;
  const allSelected = paths.length > 0 && selectedCount === paths.length;
  const someSelected = selectedCount > 0 && !allSelected;
  const risk = RISK_STYLES[category.risk];

  useEffect(() => {
    if (checkboxRef.current) {
      checkboxRef.current.indeterminate = someSelected;
    }
  }, [someSelected]);

  return (
    <div className={category.cleanable ? "bg-white/40" : "bg-slate-50/70"}>
      <div className="flex flex-wrap items-center gap-3 px-4 py-3">
        {category.cleanable ? (
          <input
            ref={checkboxRef}
            type="checkbox"
            checked={allSelected}
            disabled={busy}
            onChange={() => onToggleCategory(paths.map((path) => path.path), !allSelected)}
            aria-label={`选择${category.label}类别`}
            className="h-5 w-5 rounded border-slate-300 accent-[var(--accent)] disabled:cursor-not-allowed disabled:opacity-50"
          />
        ) : (
          <span
            className="inline-flex h-5 w-5 items-center justify-center rounded border border-slate-200 bg-slate-50 text-[10px] font-medium text-slate-400"
            title="仅展示占用，不提供删除入口"
            aria-hidden="true"
          >
            —
          </span>
        )}

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium text-slate-800">{category.label}</span>
            <span
              className={`rounded-md border px-1.5 py-0.5 text-[11px] font-medium ${risk.className}`}
            >
              {risk.label}
            </span>
            {category.cleanable ? (
              selectedCount > 0 ? (
                <span className="text-[11px] text-slate-500">
                  已选 {selectedCount}/{paths.length}
                </span>
              ) : null
            ) : (
              <span className="rounded-md border border-slate-200 bg-white px-1.5 py-0.5 text-[11px] text-slate-500">
                仅展示
              </span>
            )}
          </div>
          <p className="mt-0.5 text-xs leading-5 text-slate-500">{category.description}</p>
        </div>

        <div className="flex flex-wrap items-center justify-end gap-2">
          <span className="font-mono text-sm font-semibold tabular-nums text-slate-800">
            {category.measured ? formatBytes(category.bytes) : "未计算"}
          </span>
          {!category.cleanable && !category.measured && paths.length > 0 && onMeasureHeavy ? (
            <button
              type="button"
              onClick={onMeasureHeavy}
              disabled={busy || heavyLoading || heavyBlocked}
              title={
                heavyBlocked ? "后台扫描任务仍在进行，完成前无法计算占用" : undefined
              }
              className="h-7 rounded-lg border border-[var(--accent)]/40 bg-white px-2 text-[11px] font-medium text-[var(--accent)] transition hover:bg-[var(--accent)]/10 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {heavyLoading ? "计算中…" : "计算占用"}
            </button>
          ) : null}
        </div>

        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          aria-expanded={expanded}
          className="h-7 rounded-lg border border-slate-300 bg-white px-2 text-[11px] font-medium text-slate-600 transition hover:bg-slate-100 focus-visible:ring-2 focus-visible:ring-[var(--accent)]/40 focus-visible:outline-none"
        >
          {expanded ? "收起" : `${paths.length} 个路径`}
        </button>
      </div>

      {expanded ? (
        <ul className="border-t border-slate-100 px-4 py-2">
          {paths.map((path) => {
            const checked = selected.has(path.path);
            return (
              <li key={path.path} className="flex flex-wrap items-center gap-2 py-1.5">
                {category.cleanable ? (
                  <input
                    type="checkbox"
                    checked={checked}
                    disabled={busy}
                    onChange={() => onTogglePath(path.path)}
                    aria-label={`选择路径 ${path.path}`}
                    className="h-4 w-4 rounded border-slate-300 accent-[var(--accent)] disabled:cursor-not-allowed disabled:opacity-50"
                  />
                ) : (
                  <span className="w-4" aria-hidden="true" />
                )}
                <span
                  className="min-w-0 flex-1 truncate font-mono text-[11px] text-slate-500"
                  title={path.path}
                >
                  {path.path}
                </span>
                {path.inUse ? (
                  <span className="rounded border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[10px] text-amber-800">
                    可能正在使用
                  </span>
                ) : null}
                <span className="font-mono text-xs tabular-nums text-slate-500">
                  {category.measured ? formatBytes(path.bytes) : "—"}
                </span>
                <button
                  type="button"
                  onClick={() => onOpenPath(path.path)}
                  disabled={busy}
                  className="text-[11px] font-medium text-[var(--accent)] hover:underline disabled:cursor-not-allowed disabled:opacity-50"
                >
                  打开
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}
