"use client";

import { useEffect, useRef, useState } from "react";
import type { ToolView } from "../types";
import { formatBytes } from "@/lib/format";
import CategorySection from "./category-section";

type ToolCacheCardProps = {
  tool: ToolView;
  totalCleanableBytes: number;
  selected: Set<string>;
  busy: boolean;
  color: string;
  heavyLoading: boolean;
  onToggleTool: (paths: string[], selectAll: boolean) => void;
  onToggleCategory: (paths: string[], selectAll: boolean) => void;
  onTogglePath: (path: string) => void;
  onOpenPath: (path: string) => void;
  onMeasureHeavy: () => void;
};

/** 工具分组卡：卡头（色点 / 名称 / 体积 / 占比条 / 打开目录 / 折叠）+ 类别行。 */
export default function ToolCacheCard({
  tool,
  totalCleanableBytes,
  selected,
  busy,
  color,
  heavyLoading,
  onToggleTool,
  onToggleCategory,
  onTogglePath,
  onOpenPath,
  onMeasureHeavy,
}: ToolCacheCardProps) {
  const [collapsed, setCollapsed] = useState(false);
  const groupCheckboxRef = useRef<HTMLInputElement | null>(null);

  const cleanableCategories = tool.categories.filter(
    (category) => category.cleanable && category.paths.length > 0,
  );
  const readonlyCategories = tool.categories.filter(
    (category) => !category.cleanable && category.paths.length > 0,
  );
  const selectablePaths = cleanableCategories.flatMap((category) =>
    category.paths.map((path) => path.path),
  );
  const totalPaths =
    selectablePaths.length +
    readonlyCategories.reduce((sum, category) => sum + category.paths.length, 0);
  const selectedCount = selectablePaths.filter((path) => selected.has(path)).length;
  const allSelected = selectablePaths.length > 0 && selectedCount === selectablePaths.length;
  const someSelected = selectedCount > 0 && !allSelected;
  const ratio =
    totalCleanableBytes > 0
      ? Math.min(100, Math.round((tool.cleanableBytes / totalCleanableBytes) * 100))
      : 0;
  const primaryRoot = tool.rootPaths[0];

  useEffect(() => {
    if (groupCheckboxRef.current) {
      groupCheckboxRef.current.indeterminate = someSelected;
    }
  }, [someSelected]);

  return (
    <article
      className="overflow-hidden rounded-2xl border border-slate-200 bg-white/75 shadow-sm transition hover:-translate-y-0.5 hover:shadow-md"
      aria-busy={busy}
    >
      <div className="flex flex-wrap items-center gap-3 px-4 py-3">
        <input
          ref={groupCheckboxRef}
          type="checkbox"
          checked={allSelected}
          disabled={busy || selectablePaths.length === 0}
          onChange={() => onToggleTool(selectablePaths, !allSelected)}
          aria-label={`选择 ${tool.name} 的全部可清理缓存`}
          className="h-5 w-5 rounded border-slate-300 accent-[var(--accent)] disabled:cursor-not-allowed disabled:opacity-50"
        />
        <span
          className="inline-block h-2.5 w-2.5 shrink-0 rounded-full"
          style={{ backgroundColor: color }}
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-baseline gap-2">
            <h2 className="text-base font-semibold text-slate-900">{tool.name}</h2>
            <span className="text-[11px] text-slate-400">{tool.vendor}</span>
            {!tool.cleanable ? (
              <span
                className="rounded-md border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[11px] font-medium text-amber-800"
                title="根目录指向敏感位置，本工具只检测、不提供清理"
              >
                仅检测
              </span>
            ) : null}
            {selectedCount > 0 ? (
              <span className="text-[11px] text-slate-500">
                已选 {selectedCount}/{selectablePaths.length}
              </span>
            ) : null}
          </div>
          <p className="mt-0.5 text-xs text-slate-500">
            {tool.cleanableBytes > 0
              ? `可清理 ${formatBytes(tool.cleanableBytes)}`
              : "已检测到 · 暂无可清理缓存"}
            {tool.displayOnlyBytes > 0
              ? ` · 另有 ${formatBytes(tool.displayOnlyBytes)} 会话与模型数据（不清理）`
              : ""}
          </p>
        </div>

        {primaryRoot ? (
          <button
            type="button"
            onClick={() => onOpenPath(primaryRoot)}
            disabled={busy}
            className="h-7 rounded-lg border border-slate-300 bg-white px-2 text-[11px] font-medium text-slate-600 transition hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-50"
          >
            打开目录
          </button>
        ) : null}
        {totalPaths >= 6 ? (
          <button
            type="button"
            onClick={() => setCollapsed((value) => !value)}
            aria-expanded={!collapsed}
            aria-label={collapsed ? "展开工具明细" : "折叠工具明细"}
            className="inline-flex h-7 w-7 items-center justify-center rounded-lg border border-slate-300 bg-white text-slate-500 transition hover:bg-slate-100"
          >
            <svg
              className={`h-4 w-4 transition-transform duration-150 ${collapsed ? "" : "rotate-180"}`}
              viewBox="0 0 20 20"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              aria-hidden="true"
            >
              <path d="M5 8l5 5 5-5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </button>
        ) : null}
      </div>

      <div className="h-[3px] w-full bg-slate-200/60" aria-hidden="true">
        <div
          className="h-[3px] rounded-full transition-[width] duration-300"
          style={{ width: `${ratio}%`, backgroundColor: color }}
        />
      </div>

      {!collapsed ? (
        <div className="divide-y divide-slate-100">
          {cleanableCategories.length > 0 ? (
            <>
              <p className="px-4 pt-3 pb-1 text-[11px] font-medium tracking-[0.15em] text-slate-400 uppercase">
                可清理缓存（勾选）
              </p>
              {cleanableCategories.map((category) => (
                <CategorySection
                  key={category.category}
                  category={category}
                  selected={selected}
                  busy={busy}
                  onToggleCategory={onToggleCategory}
                  onTogglePath={onTogglePath}
                  onOpenPath={onOpenPath}
                  onMeasureHeavy={onMeasureHeavy}
                  heavyLoading={heavyLoading}
                />
              ))}
            </>
          ) : (
            <p className="px-4 py-3 text-sm text-slate-500">已检测到 · 暂无可清理缓存</p>
          )}

          {readonlyCategories.length > 0 ? (
            <>
              <p className="px-4 pt-3 pb-1 text-[11px] font-medium tracking-[0.15em] text-slate-400 uppercase">
                {tool.cleanable ? "会话与模型数据（仅展示，不清理）" : "仅检测（不清理）"}
              </p>
              {readonlyCategories.map((category) => (
                <CategorySection
                  key={category.category}
                  category={category}
                  selected={selected}
                  busy={busy}
                  onToggleCategory={onToggleCategory}
                  onTogglePath={onTogglePath}
                  onOpenPath={onOpenPath}
                  onMeasureHeavy={onMeasureHeavy}
                  heavyLoading={heavyLoading}
                />
              ))}
            </>
          ) : null}

          {tool.warnings.length > 0 ? (
            <p className="px-4 py-2 text-[11px] leading-5 text-amber-700">
              {tool.warnings.slice(0, 2).join("；")}
              {tool.warnings.length > 2 ? `；另有 ${tool.warnings.length - 2} 条提示` : ""}
            </p>
          ) : null}
        </div>
      ) : null}
    </article>
  );
}
