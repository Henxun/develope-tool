"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  cleanAiCaches,
  getAiCacheCatalog,
  invokeTauri,
  scanAiCacheUsage,
  type AiCacheCatalog,
  type AiCacheCategory,
  type AiCacheCategoryInfo,
  type AiCacheCleanProgressEvent,
  type AiCacheCleanResult,
  type AiCacheCleanTarget,
  type AiCacheScanProgressEvent,
  type AiCacheScanResult,
} from "@/lib/tauri";
import { formatBytes, formatClock, formatDuration } from "@/lib/format";
import CleanConfirmDialog from "./components/clean-confirm-dialog";
import CleanResultPanel from "./components/clean-result-panel";
import ScanProgressBar from "./components/scan-progress-bar";
import ToolCacheCard from "./components/tool-cache-card";
import type { ToolView } from "./types";

/** 工具身份色（复用 disk-heatmap 色板，只做小面积点缀）。 */
const TOOL_COLORS: Record<string, string> = {
  "claude-code": "#f3b15b",
  "claude-desktop": "#f3b15b",
  codex: "#8cc98a",
  cursor: "#7fa8d4",
  "copilot-vscode": "#b8a894",
  "copilot-cli": "#b8a894",
  windsurf: "#7fa8d4",
  "gemini-cli": "#8cc98a",
  ollama: "#b8a894",
};

const DEFAULT_TOOL_COLOR = "#b8a894";

const CATEGORY_ORDER: AiCacheCategory[] = ["cache", "logs", "sessions", "model_cache"];

type ScanMode = "initial" | "manual" | "refresh";

function extractErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return fallback;
}

/** 同一工具多根目录（VS Code 系）按类别合并，再按可清理体积排序。 */
function buildToolViews(scan: AiCacheScanResult): ToolView[] {
  return scan.tools
    .filter((tool) => tool.detected)
    .map((tool) => {
      const merged = new Map<AiCacheCategory, AiCacheCategoryInfo>();
      for (const root of tool.roots) {
        if (!root.exists) continue;
        for (const category of root.categories) {
          if (category.paths.length === 0) continue;
          const existing = merged.get(category.category);
          if (existing) {
            existing.paths = existing.paths.concat(category.paths);
            existing.bytes += category.bytes;
          } else {
            merged.set(category.category, { ...category, paths: [...category.paths] });
          }
        }
      }
      const categories: AiCacheCategoryInfo[] = [];
      for (const key of CATEGORY_ORDER) {
        const category = merged.get(key);
        if (category) categories.push(category);
      }
      const rootPaths = tool.roots
        .filter((root) => root.exists)
        .map((root) => root.path);
      return { ...tool, categories, rootPaths };
    })
    .sort((a, b) => {
      if (b.cleanableBytes !== a.cleanableBytes) {
        return b.cleanableBytes - a.cleanableBytes;
      }
      return a.name.localeCompare(b.name, "zh-Hans-CN");
    });
}

/** 默认勾选：可清理类别里、最近 10 分钟没有修改的路径。 */
function buildDefaultSelection(result: AiCacheScanResult): Set<string> {
  const next = new Set<string>();
  for (const tool of result.tools) {
    if (!tool.detected || !tool.cleanable) continue;
    for (const root of tool.roots) {
      if (!root.exists) continue;
      for (const category of root.categories) {
        if (!category.cleanable) continue;
        for (const path of category.paths) {
          if (!path.inUse) next.add(path.path);
        }
      }
    }
  }
  return next;
}

/**
 * 后端单次清理最多 64 项（`clean.rs` 的 `MAX_TARGETS_PER_REQUEST`，规格 R10）。
 * 前端按 64 分批串行提交，避免整批被拒；预览（dry-run）同样分批。
 */
const CLEAN_BATCH_SIZE = 64;

function chunkTargets(
  targets: AiCacheCleanTarget[],
  size: number,
): AiCacheCleanTarget[][] {
  const batches: AiCacheCleanTarget[][] = [];
  for (let index = 0; index < targets.length; index += size) {
    batches.push(targets.slice(index, index + size));
  }
  return batches;
}

function emptyCleanResult(dryRun: boolean): AiCacheCleanResult {
  return {
    dryRun,
    freedBytes: 0,
    deletedCount: 0,
    skippedCount: 0,
    failedCount: 0,
    items: [],
    warnings: [],
  };
}

type CleanBatchHooks = {
  /** 每批开始：用于重置单批进度事件。 */
  onBatchStart?: (batchIndex: number, batchTotal: number) => void;
  /** 每批结束：done = 已完成的目标数（含跳过/失败）。 */
  onBatchDone?: (done: number, merged: AiCacheCleanResult) => void;
};

type CleanBatchFailure = {
  batchIndex: number;
  batchTotal: number;
  message: string;
};

type CleanBatchOutcome = {
  /** 已完成批次合并后的结果（失败批次不包含在计数里）。 */
  result: AiCacheCleanResult;
  /** 某一批命令级失败时非空；此时前面批次已经真实生效。 */
  failure: CleanBatchFailure | null;
};

async function runCleanBatches(
  targets: AiCacheCleanTarget[],
  dryRun: boolean,
  hooks: CleanBatchHooks = {},
): Promise<CleanBatchOutcome> {
  const batches = chunkTargets(targets, CLEAN_BATCH_SIZE);
  let merged = emptyCleanResult(dryRun);
  let done = 0;
  for (let index = 0; index < batches.length; index += 1) {
    const batch = batches[index];
    hooks.onBatchStart?.(index + 1, batches.length);
    let result: AiCacheCleanResult;
    try {
      result = await cleanAiCaches({ targets: batch, dryRun });
    } catch (error) {
      // 命令级失败：保留前面批次已经产生的释放量，交给调用方兜底展示。
      return {
        result: merged,
        failure: {
          batchIndex: index + 1,
          batchTotal: batches.length,
          message: extractErrorMessage(error, "清理批次失败"),
        },
      };
    }
    merged = {
      ...merged,
      freedBytes: merged.freedBytes + result.freedBytes,
      deletedCount: merged.deletedCount + result.deletedCount,
      skippedCount: merged.skippedCount + result.skippedCount,
      failedCount: merged.failedCount + result.failedCount,
      items: [...merged.items, ...result.items],
      warnings: [...merged.warnings, ...result.warnings],
    };
    done += batch.length;
    hooks.onBatchDone?.(done, merged);
  }
  return { result: merged, failure: null };
}

function Stat({
  label,
  value,
  hint,
  accent = false,
}: {
  label: string;
  value: string;
  hint?: string;
  accent?: boolean;
}) {
  return (
    <div className="relative overflow-hidden rounded-xl border border-slate-200 bg-white/75 px-4 py-3 shadow-sm">
      {accent ? (
        <span
          className="absolute top-2 bottom-2 left-0 w-[3px] rounded-full bg-[var(--accent)]"
          aria-hidden="true"
        />
      ) : null}
      <div className={accent ? "pl-2" : ""}>
        <div className="text-xs font-medium text-slate-400">{label}</div>
        <div className="mt-0.5 font-mono text-xl font-semibold tabular-nums text-slate-800">
          {value}
        </div>
        {hint ? <div className="mt-0.5 text-[11px] text-slate-400">{hint}</div> : null}
      </div>
    </div>
  );
}

function SkeletonCards() {
  return (
    <div className="space-y-3" aria-hidden="true">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        {Array.from({ length: 4 }).map((_, index) => (
          <div
            key={index}
            className="h-[68px] animate-pulse rounded-xl border border-slate-200 bg-white/70"
          />
        ))}
      </div>
      {Array.from({ length: 3 }).map((_, index) => (
        <div
          key={index}
          className="animate-pulse rounded-2xl border border-slate-200 bg-white/70 p-4"
        >
          <div className="h-5 w-40 rounded bg-slate-200" />
          <div className="mt-3 space-y-2">
            <div className="h-4 w-[70%] rounded bg-slate-200" />
            <div className="h-4 w-[88%] rounded bg-slate-200" />
            <div className="h-4 w-[62%] rounded bg-slate-200" />
          </div>
        </div>
      ))}
    </div>
  );
}

export default function AiCacheCleanPage() {
  const [catalog, setCatalog] = useState<AiCacheCatalog | null>(null);
  const [scan, setScan] = useState<AiCacheScanResult | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [scanning, setScanning] = useState(false);
  /** 后端扫描任务是否仍未结束（「停止显示结果」只是不再等待，任务仍在跑）。 */
  const [scanTaskPending, setScanTaskPending] = useState(false);
  const [progress, setProgress] = useState<AiCacheScanProgressEvent | null>(null);
  const [scanStartedAt, setScanStartedAt] = useState(0);
  const [elapsedMs, setElapsedMs] = useState(0);
  const [lastScanAt, setLastScanAt] = useState(0);
  const [scanDurationMs, setScanDurationMs] = useState(0);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [showSupported, setShowSupported] = useState(false);

  const [preview, setPreview] = useState<AiCacheCleanResult | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [cleanProgress, setCleanProgress] = useState<AiCacheCleanProgressEvent | null>(null);
  const [cleanResult, setCleanResult] = useState<AiCacheCleanResult | null>(null);
  /** 部分批次命令级失败时的提示（明确告知前几批已生效）。 */
  const [cleanFailureNotice, setCleanFailureNotice] = useState("");
  const [cleanTotal, setCleanTotal] = useState(0);
  /** 已完成批次的项数（含跳过/失败），与当前批已处理数相加作为进度分子。 */
  const [cleanDone, setCleanDone] = useState(0);
  /** 当前清理批次（>64 项时前端分批串行提交）。 */
  const [cleanBatch, setCleanBatch] = useState<{ index: number; total: number } | null>(null);
  const [cleanDurationMs, setCleanDurationMs] = useState(0);
  /** 正在「计算占用」的工具 id（展示类别惰性计算，互不阻塞）。 */
  const [heavyLoading, setHeavyLoading] = useState<Set<string>>(new Set());

  const scanTokenRef = useRef(0);
  const refreshTimerRef = useRef(0);
  const scanTaskPendingRef = useRef(false);

  const runScan = useCallback(async (mode: ScanMode) => {
    // 后端扫描不可中断：旧任务结束前不再发起新扫描，避免并发扫描。
    if (scanTaskPendingRef.current) return;
    scanTaskPendingRef.current = true;
    setScanTaskPending(true);
    const token = ++scanTokenRef.current;
    const isRefresh = mode === "refresh";
    setScanning(true);
    setError("");
    setProgress(null);
    setScanStartedAt(Date.now());
    setElapsedMs(0);
    if (isRefresh) {
      setNotice("正在刷新缓存状态…");
    } else {
      setScan(null);
      setSelected(new Set());
      setPreview(null);
      setCleanResult(null);
      setCleanProgress(null);
      setNotice("");
    }

    try {
      const result = await scanAiCacheUsage();
      if (token !== scanTokenRef.current) return;
      setScan(result);
      setLastScanAt(Date.now());
      setScanDurationMs(result.durationMs);
      setSelected(buildDefaultSelection(result));
      if (isRefresh) setNotice("");
    } catch (scanError) {
      if (token !== scanTokenRef.current) return;
      setError(extractErrorMessage(scanError, "扫描失败"));
      if (isRefresh) setNotice("");
    } finally {
      scanTaskPendingRef.current = false;
      setScanTaskPending(false);
      if (token === scanTokenRef.current) setScanning(false);
    }
  }, []);

  // 进入页面：先取 catalog（毫秒级），300ms 后自动扫描，避开路由入场。
  useEffect(() => {
    let cancelled = false;
    const loadCatalog = async () => {
      try {
        const result = await getAiCacheCatalog();
        if (!cancelled) setCatalog(result);
      } catch (catalogError) {
        if (!cancelled) {
          setError((current) => current || extractErrorMessage(catalogError, "读取 AI 工具清单失败"));
        }
      }
    };
    void loadCatalog();
    const timer = window.setTimeout(() => {
      void runScan("initial");
    }, 300);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      scanTokenRef.current += 1;
      if (refreshTimerRef.current) window.clearTimeout(refreshTimerRef.current);
    };
  }, [runScan]);

  // 进度事件（扫描按工具节流、清理按项）。不支持 Tauri 的浏览器环境直接跳过。
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    let unlistenScan: (() => void) | undefined;
    let unlistenClean: (() => void) | undefined;
    let closed = false;

    const setup = async () => {
      unlistenScan = await listen<AiCacheScanProgressEvent>(
        "ai-cache-scan-progress",
        (event) => {
          if (!closed) setProgress(event.payload);
        },
      );
      unlistenClean = await listen<AiCacheCleanProgressEvent>(
        "ai-cache-clean-progress",
        (event) => {
          if (!closed) setCleanProgress(event.payload);
        },
      );
      if (closed) {
        unlistenScan?.();
        unlistenClean?.();
      }
    };
    void setup();

    return () => {
      closed = true;
      unlistenScan?.();
      unlistenClean?.();
    };
  }, []);

  // 扫描计时器。
  useEffect(() => {
    if (!scanning) return;
    const timer = window.setInterval(() => {
      setElapsedMs(Date.now() - scanStartedAt);
    }, 500);
    return () => window.clearInterval(timer);
  }, [scanning, scanStartedAt]);

  const toolViews = useMemo(() => (scan ? buildToolViews(scan) : []), [scan]);

  const toolNameById = useMemo(() => {
    const map: Record<string, string> = {};
    for (const tool of scan?.tools ?? []) map[tool.id] = tool.name;
    return map;
  }, [scan]);

  const cleanableItemCount = useMemo(() => {
    if (!scan) return 0;
    let count = 0;
    for (const tool of scan.tools) {
      if (!tool.cleanable) continue;
      for (const root of tool.roots) {
        for (const category of root.categories) {
          if (category.cleanable) count += category.paths.length;
        }
      }
    }
    return count;
  }, [scan]);

  const selectedTargets = useMemo<AiCacheCleanTarget[]>(() => {
    if (!scan) return [];
    const targets: AiCacheCleanTarget[] = [];
    for (const tool of scan.tools) {
      if (!tool.detected || !tool.cleanable) continue;
      for (const root of tool.roots) {
        if (!root.exists) continue;
        for (const category of root.categories) {
          if (!category.cleanable) continue;
          for (const path of category.paths) {
            if (selected.has(path.path)) {
              targets.push({ toolId: tool.id, category: category.category, path: path.path });
            }
          }
        }
      }
    }
    return targets;
  }, [scan, selected]);

  const selectedBytes = useMemo(() => {
    if (!scan) return 0;
    let total = 0;
    for (const tool of scan.tools) {
      if (!tool.cleanable) continue;
      for (const root of tool.roots) {
        for (const category of root.categories) {
          if (!category.cleanable) continue;
          for (const path of category.paths) {
            if (selected.has(path.path)) total += path.bytes;
          }
        }
      }
    }
    return total;
  }, [scan, selected]);

  const selectedInUsePaths = useMemo(() => {
    if (!scan) return [];
    const paths: string[] = [];
    for (const tool of scan.tools) {
      for (const root of tool.roots) {
        for (const category of root.categories) {
          if (!category.cleanable) continue;
          for (const path of category.paths) {
            if (path.inUse && selected.has(path.path)) paths.push(path.path);
          }
        }
      }
    }
    return paths;
  }, [scan, selected]);

  const checkedPaths = useMemo(() => {
    const paths: string[] = [];
    for (const tool of catalog?.tools ?? []) {
      for (const root of tool.roots) {
        if (!paths.includes(root)) paths.push(root);
      }
    }
    return paths;
  }, [catalog]);

  const protectedByTool = useMemo(() => {
    const map: Record<string, string[]> = {};
    for (const tool of catalog?.tools ?? []) map[tool.id] = tool.protected;
    return map;
  }, [catalog]);

  const supportedToolNames = useMemo(() => {
    const names = (catalog?.tools ?? []).map((tool) => tool.name);
    return names.length > 0
      ? names
      : [
          "Claude Code",
          "Claude Desktop",
          "Codex CLI",
          "Cursor",
          "GitHub Copilot",
          "Windsurf",
          "Gemini CLI",
          "Ollama",
        ];
  }, [catalog]);

  const busy = scanning || cleaning || previewLoading;

  const setPathsSelection = useCallback((paths: string[], selectAll: boolean) => {
    setSelected((current) => {
      const next = new Set(current);
      for (const path of paths) {
        if (selectAll) next.add(path);
        else next.delete(path);
      }
      return next;
    });
  }, []);

  const handleTogglePath = useCallback((path: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const handleSelectAllSafe = useCallback(() => {
    if (!scan) return;
    const paths: string[] = [];
    for (const tool of scan.tools) {
      if (!tool.cleanable) continue;
      for (const root of tool.roots) {
        for (const category of root.categories) {
          if (!category.cleanable) continue;
          for (const path of category.paths) {
            if (!path.inUse) paths.push(path.path);
          }
        }
      }
    }
    setPathsSelection(paths, true);
  }, [scan, setPathsSelection]);

  const handleClearSelection = useCallback(() => setSelected(new Set()), []);

  const handleCancelScan = useCallback(() => {
    scanTokenRef.current += 1;
    setScanning(false);
    setProgress(null);
    setNotice("已停止显示结果；后端扫描仍会在后台完成，稍后可点「重新扫描」刷新。");
  }, []);

  const handleOpenPath = useCallback(async (path: string) => {
    try {
      await invokeTauri("open_path", { path });
    } catch (openError) {
      setError(extractErrorMessage(openError, "打开目录失败"));
    }
  }, []);

  /** 「计算占用」：只对该工具补扫展示类别（sessions / model_cache），结果合并回原卡。 */
  const handleMeasureHeavy = useCallback(async (toolId: string) => {
    // 后端扫描不可中断：旧任务结束前不再发起新的扫描请求，避免并发扫描。
    if (scanTaskPendingRef.current) {
      setNotice("后台扫描仍在进行，完成前无法计算占用。");
      return;
    }
    const token = scanTokenRef.current;
    setError("");
    setHeavyLoading((current) => new Set(current).add(toolId));
    try {
      const result = await scanAiCacheUsage([toolId], true);
      if (token !== scanTokenRef.current) return;
      const measured = result.tools[0];
      if (!measured) return;
      setScan((current) => {
        if (!current) return current;
        const tools = current.tools.map((tool) => (tool.id === toolId ? measured : tool));
        return {
          ...current,
          tools,
          totalCleanableBytes: tools.reduce((sum, tool) => sum + tool.cleanableBytes, 0),
          totalDisplayOnlyBytes: tools.reduce((sum, tool) => sum + tool.displayOnlyBytes, 0),
        };
      });
    } catch (measureError) {
      if (token === scanTokenRef.current) {
        setError(extractErrorMessage(measureError, "计算占用失败"));
      }
    } finally {
      setHeavyLoading((current) => {
        const next = new Set(current);
        next.delete(toolId);
        return next;
      });
    }
  }, []);

  const handleRequestClean = useCallback(async () => {
    if (busy || selectedTargets.length === 0) return;
    setError("");
    setPreviewLoading(true);
    try {
      const outcome = await runCleanBatches(selectedTargets, true);
      if (outcome.failure) {
        setError(
          `第 ${outcome.failure.batchIndex}/${outcome.failure.batchTotal} 批预览失败，未执行任何删除：${outcome.failure.message}`,
        );
        return;
      }
      setPreview(outcome.result);
    } finally {
      setPreviewLoading(false);
    }
  }, [busy, selectedTargets]);

  const handleConfirmClean = useCallback(async () => {
    if (cleaning) return;
    const targets = selectedTargets;
    if (targets.length === 0) {
      setPreview(null);
      return;
    }
    setPreview(null);
    setCleaning(true);
    setCleanResult(null);
    setCleanProgress(null);
    setCleanTotal(targets.length);
    setCleanDone(0);
    setCleanBatch(null);
    setCleanFailureNotice("");
    setError("");
    const startedAt = Date.now();

    try {
      const outcome = await runCleanBatches(targets, false, {
        onBatchStart: (batchIndex, batchTotal) => {
          setCleanProgress(null);
          setCleanBatch({ index: batchIndex, total: batchTotal });
        },
        onBatchDone: (done) => setCleanDone(done),
      });
      // 无论中途是否失败，已完成的批次都真实生效：先合并展示，再给出失败说明。
      setCleanResult(outcome.result);
      setCleanDurationMs(Date.now() - startedAt);
      setSelected(new Set());
      if (outcome.failure) {
        const doneBatches = outcome.failure.batchIndex - 1;
        setCleanFailureNotice(
          `第 ${outcome.failure.batchIndex}/${outcome.failure.batchTotal} 批清理失败，前 ${doneBatches} 批已生效：${outcome.failure.message}`,
        );
      } else {
        setCleanFailureNotice("");
      }
      if (refreshTimerRef.current) window.clearTimeout(refreshTimerRef.current);
      refreshTimerRef.current = window.setTimeout(() => {
        void runScan("refresh");
      }, 800);
    } catch (cleanError) {
      setError(extractErrorMessage(cleanError, "清理失败"));
    } finally {
      setCleaning(false);
      setCleanBatch(null);
    }
  }, [cleaning, selectedTargets, runScan]);

  /**
   * 进度分子 = 已完成批次项数 + 当前批已处理数。
   * `processedCount` 含删除/跳过/失败，跳过项不会让进度条停在 100% 之前。
   */
  const cleanDoneCount =
    cleanDone + (cleanProgress?.processedCount ?? cleanProgress?.deletedCount ?? 0);
  const cleanPercent =
    cleanTotal > 0 ? Math.min(100, Math.round((cleanDoneCount / cleanTotal) * 100)) : 0;

  const showEmpty = Boolean(scan) && !scanning && toolViews.length === 0;
  const totalCleanableBytes = scan?.totalCleanableBytes ?? 0;

  return (
    <main className="rounded-3xl border border-[var(--card-border)] bg-[var(--card)] p-6 shadow-[0_24px_65px_-35px_rgba(17,97,125,0.55)] backdrop-blur-xl sm:p-8">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <p className="font-mono text-[11px] tracking-[0.2em] text-[var(--accent)] uppercase">
            AI CACHE CLEANER
          </p>
          <h1 className="mt-2 text-3xl font-semibold tracking-tight">AI 智能体缓存清理</h1>
          <p className="mt-2 max-w-3xl text-sm leading-6 text-slate-700">
            扫描 Claude Code、Codex CLI、Cursor 等 AI 编码工具在本机留下的缓存与临时数据，按工具分组查看占用，勾选后清理。
          </p>
        </div>
        <div className="flex flex-col items-end gap-1.5">
          <div className="flex items-center gap-2">
            {previewLoading ? (
              <span className="flex items-center gap-2 text-xs text-slate-500">
                <span className="inline-block h-3.5 w-3.5 animate-spin rounded-full border-2 border-slate-300 border-t-slate-600" />
                生成清理预览中…
              </span>
            ) : null}
            {scanning ? (
              <button
                type="button"
                onClick={handleCancelScan}
                title="只停止界面显示与计时，后端扫描任务仍会跑完"
                className="h-9 rounded-xl border border-slate-300 bg-white/80 px-3 text-sm font-medium text-slate-700 transition hover:bg-white"
              >
                停止显示结果
              </button>
            ) : (
              <button
                type="button"
                onClick={() => void runScan("manual")}
                disabled={cleaning || previewLoading || scanTaskPending || heavyLoading.size > 0}
                title={scanTaskPending ? "后台扫描任务仍在进行，完成前无法重新扫描" : undefined}
                className="h-9 rounded-xl bg-[var(--accent)] px-3 text-sm font-medium text-white transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
              >
                重新扫描
              </button>
            )}
          </div>
          {scanning ? (
            <p className="text-[11px] text-slate-400">停止显示只影响界面，后端扫描仍会完成</p>
          ) : null}
          {scanTaskPending && !scanning ? (
            <p className="text-[11px] text-amber-600">
              后台扫描仍在进行，完成前无法重新扫描或计算占用。
            </p>
          ) : null}
          <p className="text-[11px] text-slate-400">
            {lastScanAt > 0 ? `上次扫描 ${formatClock(lastScanAt)}` : "尚未扫描"}
            {scanDurationMs > 0 ? ` · 用时 ${formatDuration(scanDurationMs)}` : ""}
          </p>
        </div>
      </div>

      <div className="mt-5 space-y-4">
        {cleanResult ? (
          <CleanResultPanel
            result={cleanResult}
            toolNameById={toolNameById}
            durationMs={cleanDurationMs}
            failureNotice={cleanFailureNotice}
            onRescan={() => void runScan("manual")}
            onDismiss={() => {
              setCleanResult(null);
              setCleanFailureNotice("");
            }}
          />
        ) : null}

        {error ? (
          <div className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-rose-200 bg-rose-50 px-4 py-3 text-sm text-rose-700">
            <span className="break-all">{error}</span>
            <button
              type="button"
              onClick={() => void runScan("manual")}
              className="text-sm font-medium underline underline-offset-2"
            >
              重试
            </button>
          </div>
        ) : null}

        {!error && notice ? (
          <div className="rounded-xl border border-sky-200 bg-sky-50 px-4 py-2.5 text-sm text-sky-800">
            {notice}
          </div>
        ) : null}

        {scanning ? (
          <ScanProgressBar progress={progress} elapsedMs={elapsedMs} />
        ) : null}

        {!scan && scanning ? <SkeletonCards /> : null}

        {scan && !showEmpty ? (
          <>
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              <Stat
                label={cleanResult ? "剩余可释放" : "可释放空间"}
                value={formatBytes(totalCleanableBytes)}
                hint={`共 ${cleanableItemCount} 项缓存`}
                accent
              />
              <Stat label="检测到的工具" value={`${toolViews.length} 个`} />
              <Stat label="缓存项" value={`${cleanableItemCount} 项`} />
              <Stat label="扫描耗时" value={formatDuration(scanDurationMs)} />
            </div>
          </>
        ) : null}

        {showEmpty ? (
          <div className="rounded-2xl border border-dashed border-slate-300 bg-white/60 p-10 text-center">
            <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl border border-slate-200 bg-white text-slate-400">
              <svg
                className="h-6 w-6"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                aria-hidden="true"
              >
                <circle cx="11" cy="11" r="6.5" />
                <path d="M16 16l4 4" strokeLinecap="round" />
              </svg>
            </div>
            <h2 className="mt-3 text-lg font-semibold text-slate-800">
              没有检测到 AI 工具的缓存
            </h2>
            <p className="mx-auto mt-2 max-w-xl text-sm leading-6 text-slate-600">
              常见位置里都没有找到缓存目录。先安装并运行一次 AI 工具，再回来扫描。
            </p>
            {checkedPaths.length > 0 ? (
              <p className="mx-auto mt-3 max-w-2xl break-all font-mono text-[11px] leading-5 text-slate-400">
                已检查：{checkedPaths.slice(0, 8).join(" · ")}
                {checkedPaths.length > 8 ? ` · 等 ${checkedPaths.length} 个位置` : ""}
              </p>
            ) : null}
            <div className="mt-4 flex flex-wrap items-center justify-center gap-2">
              <button
                type="button"
                onClick={() => void runScan("manual")}
                className="h-9 rounded-xl bg-[var(--accent)] px-3 text-sm font-medium text-white transition hover:opacity-90"
              >
                重新扫描
              </button>
              <button
                type="button"
                onClick={() => setShowSupported((value) => !value)}
                aria-expanded={showSupported}
                className="h-9 rounded-xl border border-slate-300 bg-white px-3 text-sm font-medium text-slate-700 transition hover:bg-slate-100"
              >
                查看支持的工具
              </button>
            </div>
            {showSupported ? (
              <p className="mx-auto mt-3 max-w-xl text-xs leading-6 text-slate-500">
                {supportedToolNames.join("、")}
              </p>
            ) : null}
          </div>
        ) : null}

        {toolViews.map((tool) => (
          <ToolCacheCard
            key={tool.id}
            tool={tool}
            totalCleanableBytes={totalCleanableBytes}
            selected={selected}
            busy={busy}
            color={TOOL_COLORS[tool.id] ?? DEFAULT_TOOL_COLOR}
            heavyLoading={heavyLoading.has(tool.id)}
            scanPending={scanTaskPending}
            protectedItems={protectedByTool[tool.id] ?? []}
            onToggleTool={setPathsSelection}
            onToggleCategory={setPathsSelection}
            onTogglePath={handleTogglePath}
            onOpenPath={(path) => void handleOpenPath(path)}
            onMeasureHeavy={() => void handleMeasureHeavy(tool.id)}
          />
        ))}

        {scan && !showEmpty && !scanning ? (
          <div className="sticky bottom-4 z-20 mt-4 rounded-2xl border border-slate-200 bg-white/85 px-4 py-3 shadow-[0_-8px_24px_-12px_rgba(15,23,42,0.18)] backdrop-blur-xl">
            {cleaning ? (
              <div>
                <div className="flex flex-wrap items-center gap-2">
                  <span className="inline-block h-4 w-4 animate-spin rounded-full border-2 border-slate-300 border-t-[var(--accent)]" />
                  <p className="text-sm font-medium text-slate-800">
                    正在清理 {cleanDoneCount}/{cleanTotal}
                    {cleanBatch && cleanBatch.total > 1
                      ? ` · 第 ${cleanBatch.index}/${cleanBatch.total} 批`
                      : ""}
                  </p>
                  <span className="min-w-0 truncate font-mono text-[11px] text-slate-500">
                    {cleanProgress?.toolId ? toolNameById[cleanProgress.toolId] ?? cleanProgress.toolId : ""}
                    {cleanProgress?.path ? ` · ${cleanProgress.path}` : ""}
                  </span>
                </div>
                <div className="mt-2 h-1.5 rounded-full bg-slate-200">
                  <div
                    className="h-1.5 rounded-full bg-[var(--accent)] transition-[width] duration-300 ease-out"
                    style={{ width: `${cleanPercent}%` }}
                  />
                </div>
              </div>
            ) : (
              <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                <p className="text-sm text-slate-700">
                  {selectedTargets.length > 0 ? (
                    <>
                      已选{" "}
                      <span className="font-mono font-semibold tabular-nums text-slate-900">
                        {selectedTargets.length}
                      </span>{" "}
                      项 · 预计释放{" "}
                      <span className="font-mono font-semibold tabular-nums text-slate-900">
                        {formatBytes(selectedBytes)}
                      </span>
                    </>
                  ) : (
                    "请勾选要清理的缓存项"
                  )}
                </p>
                <div className="flex flex-wrap items-center gap-2">
                  <button
                    type="button"
                    onClick={handleSelectAllSafe}
                    disabled={busy}
                    className="h-9 rounded-lg border border-slate-300 bg-white px-2.5 text-xs font-medium text-slate-600 transition hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    全选安全项
                  </button>
                  <button
                    type="button"
                    onClick={handleClearSelection}
                    disabled={busy || selectedTargets.length === 0}
                    className="h-9 rounded-lg border border-slate-300 bg-white px-2.5 text-xs font-medium text-slate-600 transition hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    清除选择
                  </button>
                  <button
                    type="button"
                    onClick={() => void handleRequestClean()}
                    disabled={busy || selectedTargets.length === 0}
                    className="h-10 rounded-xl bg-rose-600 px-4 text-sm font-medium text-white transition hover:bg-rose-700 focus-visible:ring-2 focus-visible:ring-rose-400 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    {previewLoading ? "生成预览中…" : `清理选中 ${selectedTargets.length} 项`}
                  </button>
                </div>
              </div>
            )}
          </div>
        ) : null}
      </div>

      {preview ? (
        <CleanConfirmDialog
          preview={preview}
          toolNameById={toolNameById}
          inUsePaths={selectedInUsePaths}
          onCancel={() => setPreview(null)}
          onConfirm={() => void handleConfirmClean()}
        />
      ) : null}
    </main>
  );
}
