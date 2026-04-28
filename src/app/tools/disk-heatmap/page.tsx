"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ResponsiveTreeMapHtml } from "@nivo/treemap";
import { listen } from "@tauri-apps/api/event";
import { invokeTauri, toSinglePath } from "@/lib/tauri";
import { open } from "@tauri-apps/plugin-dialog";

// ── Types ──

type DiskNode = {
  name: string;
  size: number;
  nodeType: string;
  children: DiskNode[];
};

type DiskScanResult = {
  root: DiskNode;
  totalSize: number;
  totalFiles: number;
  totalDirs: number;
  scanDurationMs: number;
};

type DiskScanProgressEvent = {
  currentPath: string;
  itemsScanned: number;
};

type NivoNode = {
  id: string;
  value?: number;
  nodeType: string;
  children?: NivoNode[];
};

// ── Color mapping ──

const CATEGORY_COLORS: Record<string, string> = {
  documents: "#4e79a7",
  images: "#f28e2b",
  video: "#e15759",
  audio: "#76b7b2",
  code: "#59a14f",
  archives: "#edc948",
  data: "#b07aa1",
  system: "#ff9da7",
  other: "#9c755f",
  folder: "#bab0ac",
};

const CATEGORY_LABELS: Record<string, string> = {
  documents: "文档",
  images: "图片",
  video: "视频",
  audio: "音频",
  code: "代码",
  archives: "压缩包",
  data: "数据",
  system: "系统",
  other: "其它",
  folder: "文件夹",
};

const DOC_EXTS = new Set(["doc", "docx", "pdf", "txt", "md", "rtf", "odt", "xls", "xlsx", "ppt", "pptx", "pages", "numbers", "key"]);
const IMG_EXTS = new Set(["jpg", "jpeg", "png", "gif", "svg", "bmp", "ico", "webp", "tiff", "tif", "heic", "heif", "raw", "psd"]);
const VID_EXTS = new Set(["mp4", "avi", "mkv", "mov", "wmv", "flv", "webm", "m4v", "3gp", "ts"]);
const AUD_EXTS = new Set(["mp3", "wav", "flac", "ogg", "aac", "wma", "m4a", "opus", "mid", "midi"]);
const CODE_EXTS = new Set(["js", "ts", "jsx", "tsx", "py", "rs", "go", "java", "c", "cpp", "h", "hpp", "cs", "rb", "php", "swift", "kt", "scala", "sh", "bat", "ps1", "lua", "r", "m", "dart", "vue", "svelte"]);
const ARC_EXTS = new Set(["zip", "tar", "gz", "bz2", "xz", "7z", "rar", "zst", "cab", "iso", "dmg", "tgz"]);
const DATA_EXTS = new Set(["json", "xml", "csv", "yaml", "yml", "toml", "ini", "cfg", "conf", "sql", "db", "sqlite"]);
const SYS_EXTS = new Set(["dll", "exe", "sys", "so", "dylib", "deb", "rpm", "msi", "app", "com", "ocx"]);

function getFileCategory(ext: string): string {
  const lower = ext.toLowerCase();
  if (DOC_EXTS.has(lower)) return "documents";
  if (IMG_EXTS.has(lower)) return "images";
  if (VID_EXTS.has(lower)) return "video";
  if (AUD_EXTS.has(lower)) return "audio";
  if (CODE_EXTS.has(lower)) return "code";
  if (ARC_EXTS.has(lower)) return "archives";
  if (DATA_EXTS.has(lower)) return "data";
  if (SYS_EXTS.has(lower)) return "system";
  return "other";
}

function getCategoryColor(nodeType: string): string {
  if (nodeType === "folder") return CATEGORY_COLORS.folder;
  return CATEGORY_COLORS[getFileCategory(nodeType)] ?? CATEGORY_COLORS.other;
}

// ── Data transformation ──

function transformToNivoData(node: DiskNode, parentPath = ""): NivoNode {
  const currentPath = parentPath ? `${parentPath}/${node.name}` : node.name;

  if (node.children.length === 0) {
    return {
      id: currentPath,
      value: node.size,
      nodeType: node.nodeType,
    };
  }

  return {
    id: currentPath,
    nodeType: node.nodeType,
    children: node.children.map((child) => transformToNivoData(child, currentPath)),
  };
}

function findChildNode(root: DiskNode, path: string[]): DiskNode | null {
  if (path.length === 0) return root;
  const child = root.children.find((c) => c.name === path[0]);
  if (!child) return null;
  return findChildNode(child, path.slice(1));
}

// ── Formatting ──

function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const idx = Math.min(i, units.length - 1);
  return `${(bytes / Math.pow(k, idx)).toFixed(1)} ${units[idx]}`;
}

function formatNumber(n: number): string {
  return n.toLocaleString("zh-CN");
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function extractErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return fallback;
}

// ── Custom treemap node ──

function TreemapNode({ node, onMouseEnter, onMouseLeave, onClick }: any) {
  const category = node.data.nodeType === "folder" ? "folder" : getFileCategory(node.data.nodeType);
  const color = CATEGORY_COLORS[category] ?? CATEGORY_COLORS.other;
  const label = node.id.split("/").pop() ?? node.id;
  const w = node.width ?? 0;
  const h = node.height ?? 0;
  const showLabel = w > 50 && h > 18;
  const clickCountRef = useRef(0);
  const clickTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleClick = useCallback(() => {
    clickCountRef.current += 1;
    if (clickCountRef.current === 1) {
      clickTimerRef.current = setTimeout(() => {
        clickCountRef.current = 0;
      }, 300);
    } else if (clickCountRef.current >= 2) {
      if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
      clickCountRef.current = 0;
      // Double click → drill into folder
      onClick?.();
    }
  }, [onClick]);

  return (
    <div
      style={{
        position: "absolute",
        left: node.x ?? 0,
        top: node.y ?? 0,
        width: w,
        height: h,
        background: color,
        border: `1px solid rgba(0,0,0,0.15)`,
        overflow: "hidden",
        cursor: node.data.nodeType === "folder" ? "pointer" : "default",
        display: "flex",
        alignItems: "flex-start",
        padding: showLabel ? "3px 5px" : "0",
        transition: "opacity 0.15s",
      }}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      onClick={node.data.nodeType === "folder" ? handleClick : undefined}
      title={`${label}${node.data.value ? ` — ${formatFileSize(node.data.value)}` : node.data.nodeType === "folder" ? ` — ${formatFileSize(node.data.size)}` : ""}`}
    >
      {showLabel && (
        <span
          style={{
            fontSize: Math.min(11, h * 0.4),
            color: "rgba(255,255,255,0.9)",
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
            lineHeight: 1.2,
            textShadow: "0 1px 2px rgba(0,0,0,0.4)",
          }}
        >
          {label}
        </span>
      )}
    </div>
  );
}

// ── Page component ──

export default function DiskHeatmapPage() {
  const [rootPath, setRootPath] = useState("");
  const [maxDepth, setMaxDepth] = useState(5);
  const [includeHidden, setIncludeHidden] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [scanResult, setScanResult] = useState<DiskScanResult | null>(null);
  const [error, setError] = useState("");
  const [progress, setProgress] = useState<DiskScanProgressEvent | null>(null);
  const [zoomPath, setZoomPath] = useState<string[]>([]);
  const [hoveredNode, setHoveredNode] = useState<any>(null);
  const [viewScale, setViewScale] = useState(1);
  const unlistenRef = useRef<(() => void) | null>(null);
  const isInsideTreemapRef = useRef(false);
  const treemapRef = useRef<HTMLDivElement>(null);

  const MIN_SCALE = 0.5;
  const MAX_SCALE = 5;
  const SCALE_STEP = 0.15;

  // Listen for scan progress events
  useEffect(() => {
    const setup = async () => {
      if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
      const unlisten = await listen<DiskScanProgressEvent>("disk-scan-progress", (event) => {
        setProgress(event.payload);
      });
      unlistenRef.current = unlisten;
    };
    setup();
    return () => {
      unlistenRef.current?.();
    };
  }, []);

  const handlePickDirectory = useCallback(async () => {
    try {
      const selected = await open({ directory: true, multiple: false });
      const path = toSinglePath(selected);
      if (path) setRootPath(path);
    } catch {
      // User cancelled
    }
  }, []);

  const handleScan = useCallback(async () => {
    if (!rootPath.trim()) {
      setError("请输入或选择要扫描的目录路径");
      return;
    }

    setScanning(true);
    setError("");
    setScanResult(null);
    setProgress(null);
    setZoomPath([]);
    setHoveredNode(null);
    setViewScale(1);

    try {
      const result = await invokeTauri<DiskScanResult>("scan_disk_usage", {
        rootPath: rootPath.trim(),
        maxDepth: maxDepth > 0 ? maxDepth : undefined,
        includeHidden,
      });
      setScanResult(result);
    } catch (e) {
      setError(extractErrorMessage(e, "扫描失败"));
    } finally {
      setScanning(false);
      setProgress(null);
    }
  }, [rootPath, maxDepth, includeHidden]);

  // Navigate the tree using zoom path
  const currentRoot = useMemo<DiskNode | null>(() => {
    if (!scanResult) return null;
    if (zoomPath.length === 0) return scanResult.root;
    return findChildNode(scanResult.root, zoomPath) ?? scanResult.root;
  }, [scanResult, zoomPath]);

  // Transform current view for Nivo
  const nivoData = useMemo(() => {
    if (!currentRoot) return null;
    return transformToNivoData(currentRoot, currentRoot.name);
  }, [currentRoot]);

  // Breadcrumb items
  const breadcrumbItems = useMemo(() => {
    if (!scanResult) return [];
    const items = [{ name: scanResult.root.name, path: [] as string[] }];
    for (let i = 0; i < zoomPath.length; i++) {
      items.push({
        name: zoomPath[i],
        path: zoomPath.slice(0, i + 1),
      });
    }
    return items;
  }, [scanResult, zoomPath]);

  const handleBreadcrumbClick = useCallback((path: string[]) => {
    setZoomPath(path);
  }, []);

  const handleNodeDoubleClick = useCallback(
    (node: any) => {
      if (!scanResult || !currentRoot) return;
      // Only drill into folder nodes
      if (node.data?.nodeType !== "folder") return;
      const childName = node.id.split("/").pop();
      if (!childName) return;
      const childNode = currentRoot.children.find((c) => c.name === childName && c.nodeType === "folder");
      if (childNode && childNode.children.length > 0) {
        setZoomPath([...zoomPath, childName]);
        setHoveredNode(null);
        setViewScale(1);
      }
    },
    [scanResult, currentRoot, zoomPath],
  );

  // ── Wheel zoom: visual scale + prevent page scroll ──

  const handleTreemapMouseEnter = useCallback(() => {
    isInsideTreemapRef.current = true;
    document.body.style.overflow = "hidden";
  }, []);

  const handleTreemapMouseLeave = useCallback(() => {
    isInsideTreemapRef.current = false;
    document.body.style.overflow = "";
  }, []);

  // Native passive:false listener to prevent page scroll + adjust scale
  useEffect(() => {
    const el = treemapRef.current;
    if (!el) return;

    const handler = (event: WheelEvent) => {
      if (!isInsideTreemapRef.current) return;
      if (!scanResult) return;
      if (Math.abs(event.deltaY) < 5) return;

      event.preventDefault();
      event.stopPropagation();

      setViewScale((prev) => {
        const delta = event.deltaY < 0 ? SCALE_STEP : -SCALE_STEP;
        const next = Math.round((prev + delta) * 100) / 100;
        return Math.min(MAX_SCALE, Math.max(MIN_SCALE, next));
      });
    };

    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  }, [scanResult]);

  // Reset scale when zoom path changes
  useEffect(() => { setViewScale(1); }, [zoomPath]);

  // Collect categories present in current view for legend
  const presentCategories = useMemo(() => {
    if (!currentRoot) return new Set<string>();
    const categories = new Set<string>();
    const walk = (node: DiskNode) => {
      if (node.nodeType === "folder") {
        categories.add("folder");
      } else {
        categories.add(getFileCategory(node.nodeType));
      }
      node.children.forEach(walk);
    };
    walk(currentRoot);
    return categories;
  }, [currentRoot]);

  return (
    <div className="space-y-5">
      {/* Header */}
      <div>
        <h2 className="text-xl font-semibold text-slate-800">磁盘分析热力图</h2>
        <p className="text-sm text-slate-500 mt-1">可视化磁盘使用分布，按文件类型着色，点击文件夹可钻入查看。</p>
      </div>

      {/* Controls */}
      <div className="bg-white rounded-xl border border-slate-200 p-5 space-y-4 shadow-sm">
        <div className="flex gap-3">
          <input
            type="text"
            value={rootPath}
            onChange={(e) => setRootPath(e.target.value)}
            placeholder="输入要扫描的目录路径，如 C:\Users 或 /home"
            className="flex-1 px-3 py-2 rounded-lg border border-slate-300 text-sm focus:outline-none focus:ring-2 focus:ring-blue-400 focus:border-transparent"
            disabled={scanning}
          />
          <button
            type="button"
            onClick={handlePickDirectory}
            disabled={scanning}
            className="px-4 py-2 rounded-lg bg-slate-100 hover:bg-slate-200 text-sm font-medium text-slate-700 transition-colors disabled:opacity-50"
          >
            选择目录
          </button>
        </div>

        <div className="flex items-center gap-6 flex-wrap">
          <label className="flex items-center gap-2 text-sm text-slate-600">
            <span className="font-medium">扫描深度:</span>
            <input
              type="range"
              min={1}
              max={15}
              value={maxDepth}
              onChange={(e) => setMaxDepth(Number(e.target.value))}
              className="w-28 accent-blue-500"
              disabled={scanning}
            />
            <span className="text-xs text-slate-400 w-6 text-right">{maxDepth}</span>
          </label>
          <label className="flex items-center gap-2 text-sm text-slate-600 cursor-pointer">
            <input
              type="checkbox"
              checked={includeHidden}
              onChange={(e) => setIncludeHidden(e.target.checked)}
              className="accent-blue-500"
              disabled={scanning}
            />
            <span>显示隐藏文件</span>
          </label>
          <button
            type="button"
            onClick={handleScan}
            disabled={scanning || !rootPath.trim()}
            className="ml-auto px-5 py-2 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {scanning ? "扫描中..." : "开始扫描"}
          </button>
        </div>
      </div>

      {/* Progress */}
      {scanning && progress && (
        <div className="bg-blue-50 border border-blue-200 rounded-lg px-4 py-3 text-sm text-blue-700 flex items-center gap-2">
          <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24">
            <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
            <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
          </svg>
          <span>
            已扫描 <strong>{formatNumber(progress.itemsScanned)}</strong> 项 — {progress.currentPath}
          </span>
        </div>
      )}

      {/* Error */}
      {error && (
        <div className="bg-red-50 border border-red-200 rounded-lg px-4 py-3 text-sm text-red-700">{error}</div>
      )}

      {/* Scan results */}
      {scanResult && (
        <>
          {/* Summary stats */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
            {[
              { label: "总大小", value: formatFileSize(scanResult.totalSize) },
              { label: "文件数", value: formatNumber(scanResult.totalFiles) },
              { label: "文件夹数", value: formatNumber(scanResult.totalDirs) },
              { label: "扫描耗时", value: formatDuration(scanResult.scanDurationMs) },
            ].map((stat) => (
              <div key={stat.label} className="bg-white rounded-lg border border-slate-200 px-4 py-3 shadow-sm">
                <div className="text-xs text-slate-400 font-medium">{stat.label}</div>
                <div className="text-lg font-semibold text-slate-800 mt-0.5">{stat.value}</div>
              </div>
            ))}
          </div>

          {/* Breadcrumb */}
          <div className="flex items-center gap-1 text-sm flex-wrap">
            <button
              type="button"
              onClick={() => setZoomPath([])}
              className="text-blue-600 hover:underline font-medium"
            >
              {scanResult.root.name}
            </button>
            {zoomPath.map((segment, i) => (
              <span key={i} className="flex items-center gap-1">
                <span className="text-slate-400">/</span>
                <button
                  type="button"
                  onClick={() => handleBreadcrumbClick(zoomPath.slice(0, i + 1))}
                  className={`hover:underline ${i === zoomPath.length - 1 ? "text-slate-700 font-medium" : "text-blue-600"}`}
                >
                  {segment}
                </button>
              </span>
            ))}
            {zoomPath.length > 0 && (
              <button
                type="button"
                onClick={() => setZoomPath(zoomPath.slice(0, -1))}
                className="ml-2 text-xs text-slate-400 hover:text-slate-600 flex items-center gap-0.5"
              >
                ← 返回上级
              </button>
            )}
          </div>
        </>
      )}

      {/* Treemap container — always in DOM, native wheel listener attached */}
      <div
        ref={treemapRef}
        className="bg-slate-900 rounded-xl overflow-auto shadow-lg relative group"
        style={{ height: nivoData && currentRoot ? 500 : 0, transition: "height 0.2s ease" }}
        onMouseEnter={handleTreemapMouseEnter}
        onMouseLeave={handleTreemapMouseLeave}
      >
        {nivoData && currentRoot && (
          <div
            style={{
              transform: `scale(${viewScale})`,
              transformOrigin: "top left",
              width: `${100 / viewScale}%`,
              height: `${100 / viewScale}%`,
              transition: "transform 0.15s ease",
            }}
          >
            <ResponsiveTreeMapHtml
              data={nivoData}
              identity="id"
              value="value"
              tile="squarify"
              margin={{ top: 2, right: 2, bottom: 2, left: 2 }}
              colors={() => ""}
              nodeComponent={TreemapNode}
              onClick={handleNodeDoubleClick}
              onMouseEnter={(node: any) => setHoveredNode(node)}
              onMouseLeave={() => setHoveredNode(null)}
              animate={false}
            />
          </div>
        )}
        {/* Zoom controls overlay */}
        {nivoData && currentRoot && (
          <div className="absolute bottom-2 right-2 flex items-center gap-2 pointer-events-none opacity-0 group-hover:opacity-100 transition-opacity duration-300">
            {zoomPath.length > 0 && (
              <span className="bg-black/60 text-white/80 text-xs px-2 py-1 rounded-md backdrop-blur-sm">
                深度 {zoomPath.length}
              </span>
            )}
            <span className="bg-black/60 text-white/70 text-xs px-2 py-1 rounded-md backdrop-blur-sm">
              {Math.round(viewScale * 100)}% 滚轮缩放 · 双击钻入
            </span>
          </div>
        )}
      </div>

      {scanResult && (
        <>
          {/* Hovered node detail */}
          {hoveredNode && (
            <div className="bg-white rounded-lg border border-slate-200 px-4 py-3 shadow-sm text-sm flex items-center gap-4">
              <span className="font-medium text-slate-700">
                {hoveredNode.id.split("/").pop()}
              </span>
              <span className="text-slate-500">
                {hoveredNode.data?.nodeType === "folder"
                  ? `文件夹 — ${formatFileSize(hoveredNode.data?.size ?? hoveredNode.value ?? 0)}`
                  : `${hoveredNode.data?.nodeType?.toUpperCase() ?? "文件"} — ${formatFileSize(hoveredNode.value ?? 0)}`}
              </span>
              <span
                className="w-3 h-3 rounded-sm inline-block"
                style={{ background: getCategoryColor(hoveredNode.data?.nodeType ?? "other") }}
              />
            </div>
          )}

          {/* Legend */}
          <div className="flex flex-wrap gap-3 text-xs text-slate-500">
            {Object.entries(CATEGORY_LABELS)
              .filter(([key]) => presentCategories.has(key))
              .map(([key, label]) => (
                <span key={key} className="flex items-center gap-1.5">
                  <span
                    className="w-3 h-3 rounded-sm inline-block"
                    style={{ background: CATEGORY_COLORS[key] }}
                  />
                  {label}
                </span>
              ))}
          </div>
        </>
      )}
    </div>
  );
}
