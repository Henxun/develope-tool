"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ResponsiveTreeMapHtml } from "@nivo/treemap";
import { listen } from "@tauri-apps/api/event";
import { invokeTauri, toSinglePath } from "@/lib/tauri";
import { open } from "@tauri-apps/plugin-dialog";

// ── Types ──

type DiskNode = {
  name: string;
  path: string;
  size: number;
  nodeType: string;
  modifiedSecs: number;
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

type FileActionResult = {
  success: boolean;
  message: string;
};

type TreemapDatum = {
  id: string;
  name: string;
  path: string;
  nodeType: string;
  size: number;
  modifiedSecs: number;
  value?: number;
  children?: TreemapDatum[];
};

// Minimal shape of the computed node Nivo hands to the custom node component.
// (Nivo's ComputedNode carries more fields; we only read these.)
interface ComputedNode {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  data: TreemapDatum;
}

type ContextMenuState = {
  x: number;
  y: number;
  node: ComputedNode;
};

// ── Color mapping ──
// Pastel palette tuned to SpaceSniffer's lighter look (soft blues/greens/etc.).

const CATEGORY_COLORS: Record<string, string> = {
  documents: "#7fa8d4",
  images: "#f3b15b",
  video: "#8fbce0",
  audio: "#86cfc9",
  code: "#8cc98a",
  archives: "#f0db8c",
  data: "#c79fc0",
  system: "#f3b6bc",
  other: "#b8a894",
  folder: "#cfd6da",
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

// Map a modified-time (epoch secs) to a red→green edge color, where the oldest
// item in the scan is red and the newest is green. Used for SpaceSniffer-style
// age accents on block edges.
function getAgeEdgeColor(modifiedSecs: number, minSecs: number, maxSecs: number): string {
  if (modifiedSecs <= 0 || maxSecs <= minSecs) return "rgba(0,0,0,0)";
  const t = Math.max(0, Math.min(1, (modifiedSecs - minSecs) / (maxSecs - minSecs)));
  // 0 (old) → red hue 0, 1 (new) → green hue 120
  const hue = Math.round(t * 120);
  return `hsl(${hue}, 80%, 45%)`;
}

// ── Data transformation ──

// Build nested Nivo data, bounded for rendering. Scanning a large drive (e.g. C:)
// can yield hundreds of thousands of nodes; rendering them all as absolutely
// positioned <div>s froze the UI after the scan finished. We bound the rendered
// DOM in three ways so render cost is independent of drive size:
//   - depth cap (maxDepth): folders past the cap become size-only leaves
//   - min-size pruning: children smaller than MIN_VISIBLE_FRACTION of the parent
//     are too small to see, so they are dropped from the visual tree
//   - per-folder child cap (MAX_CHILDREN_PER_NODE): only the largest N children
//     are kept; the remainder are folded into one synthetic "其它 (...)" leaf
const MIN_VISIBLE_FRACTION = 0.001; // 0.1% of parent — below this is invisible anyway
const MAX_CHILDREN_PER_NODE = 80;

function transformToNivoData(node: DiskNode, depth: number, maxDepth: number): TreemapDatum {
  const base = {
    id: node.path,
    name: node.name,
    path: node.path,
    nodeType: node.nodeType,
    size: node.size,
    modifiedSecs: node.modifiedSecs,
  };

  if (node.children.length === 0 || depth >= maxDepth) {
    return { ...base, value: node.size };
  }

  // Children are already sorted largest-first by the backend. Keep only those
  // big enough to be visible, then cap the count; fold the rest into "其它".
  const threshold = node.size * MIN_VISIBLE_FRACTION;
  const visible: DiskNode[] = [];
  let hiddenSize = 0;
  let hiddenCount = 0;

  for (const child of node.children) {
    if (visible.length < MAX_CHILDREN_PER_NODE && child.size >= threshold) {
      visible.push(child);
    } else {
      hiddenSize += child.size;
      hiddenCount += 1;
    }
  }

  if (visible.length === 0) {
    // Nothing individually visible — render this folder as a single leaf.
    return { ...base, value: node.size };
  }

  const children: TreemapDatum[] = visible.map((child) =>
    transformToNivoData(child, depth + 1, maxDepth),
  );

  if (hiddenCount > 0 && hiddenSize > 0) {
    children.push({
      id: `${node.path}::__other__`,
      name: `其它 (${hiddenCount} 项)`,
      path: node.path,
      nodeType: "other",
      size: hiddenSize,
      modifiedSecs: 0,
      value: hiddenSize,
    });
  }

  return { ...base, children };
}

// Min/max modified-time across the (bounded) rendered tree, for age-edge
// normalization. Walks the pruned TreemapDatum tree — not the raw scan tree —
// so it stays cheap even on huge drives.
function modifiedRange(node: TreemapDatum): { min: number; max: number } {
  let min = Infinity;
  let max = 0;
  const walk = (n: TreemapDatum) => {
    if (n.modifiedSecs > 0) {
      if (n.modifiedSecs < min) min = n.modifiedSecs;
      if (n.modifiedSecs > max) max = n.modifiedSecs;
    }
    n.children?.forEach(walk);
  };
  walk(node);
  if (!isFinite(min)) min = 0;
  return { min, max };
}

function findChildNode(root: DiskNode, path: string[]): DiskNode | null {
  if (path.length === 0) return root;
  const child = root.children.find((c) => c.name === path[0]);
  if (!child) return null;
  return findChildNode(child, path.slice(1));
}

// Combined SpaceSniffer-style filter. Terms are ';'-separated and AND-combined:
//   - name globs:  *.mp4, GOPR*, ?ello.txt   (also bare substrings, e.g. "report")
//   - size compare: >500mb, <1gb, >=2.5GB     (units: b/kb/mb/gb/tb, default bytes)
// Returns a predicate over a leaf (name + size), or null when the query is empty.
type LeafPredicate = (name: string, size: number) => boolean;

const SIZE_UNITS: Record<string, number> = {
  b: 1,
  kb: 1024,
  mb: 1024 ** 2,
  gb: 1024 ** 3,
  tb: 1024 ** 4,
};

function globToRegExp(glob: string): RegExp | null {
  const escaped = glob.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".");
  try {
    return new RegExp(`^${escaped}$`, "i");
  } catch {
    return null;
  }
}

function parseFilterQuery(query: string): LeafPredicate | null {
  const terms = query
    .split(";")
    .map((t) => t.trim())
    .filter(Boolean);
  if (terms.length === 0) return null;

  const predicates: LeafPredicate[] = [];

  for (const term of terms) {
    const sizeMatch = term.match(/^(>=|<=|>|<)\s*([\d.]+)\s*(b|kb|mb|gb|tb)?$/i);
    if (sizeMatch) {
      const [, op, numStr, unitRaw] = sizeMatch;
      const num = parseFloat(numStr);
      if (!isFinite(num)) continue;
      const unit = (unitRaw ?? "b").toLowerCase();
      const threshold = num * (SIZE_UNITS[unit] ?? 1);
      predicates.push((_name, size) => {
        switch (op) {
          case ">":
            return size > threshold;
          case ">=":
            return size >= threshold;
          case "<":
            return size < threshold;
          case "<=":
            return size <= threshold;
          default:
            return true;
        }
      });
      continue;
    }

    // Name term: glob if it contains * or ?, else substring.
    if (term.includes("*") || term.includes("?")) {
      const re = globToRegExp(term);
      if (re) predicates.push((name) => re.test(name));
    } else {
      const lower = term.toLowerCase();
      predicates.push((name) => name.toLowerCase().includes(lower));
    }
  }

  if (predicates.length === 0) return null;
  return (name, size) => predicates.every((p) => p(name, size));
}

// Remove a node by path and recompute ancestor sizes (no rescan).
function removeNodeByPath(node: DiskNode, targetPath: string): { node: DiskNode; changed: boolean } {
  if (node.children.length === 0) return { node, changed: false };
  let changed = false;
  const newChildren: DiskNode[] = [];
  for (const child of node.children) {
    if (child.path === targetPath) {
      changed = true;
      continue;
    }
    const result = removeNodeByPath(child, targetPath);
    if (result.changed) changed = true;
    newChildren.push(result.node);
  }
  if (!changed) return { node, changed: false };
  const newSize = newChildren.reduce((sum, c) => sum + c.size, 0);
  return { node: { ...node, children: newChildren, size: newSize }, changed: true };
}

function countItems(node: DiskNode): { files: number; dirs: number } {
  let files = 0;
  let dirs = 0;
  for (const child of node.children) {
    if (child.nodeType === "folder") {
      dirs += 1;
      const sub = countItems(child);
      files += sub.files;
      dirs += sub.dirs;
    } else {
      files += 1;
    }
  }
  return { files, dirs };
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

// ── Custom treemap node (cushioned, SpaceSniffer-style) ──

interface TreemapNodeProps {
  node: ComputedNode;
  dim: boolean;
  selected: boolean;
  ageColor: string;
  onZoomIn: (node: ComputedNode) => void;
  onContext: (event: React.MouseEvent, node: ComputedNode) => void;
  onHoverEnter: (node: ComputedNode) => void;
  onHoverLeave: () => void;
}

function TreemapNode({
  node,
  dim,
  selected,
  ageColor,
  onZoomIn,
  onContext,
  onHoverEnter,
  onHoverLeave,
}: TreemapNodeProps) {
  const isFolder = node.data.nodeType === "folder";
  const color = getCategoryColor(node.data.nodeType);
  const label = node.data.name;
  const w = node.width ?? 0;
  const h = node.height ?? 0;
  const showName = w > 46 && h > 16;
  const showSize = w > 60 && h > 30;

  // Single left-click on a folder drills in.
  const handleClick = useCallback(() => {
    onZoomIn(node);
  }, [node, onZoomIn]);

  // Cushion effect: a soft top-left highlight + bottom-right shade over the base color.
  // Selected block goes bright white (SpaceSniffer selection look).
  const cushion = selected
    ? "radial-gradient(120% 120% at 30% 24%, #ffffff 0%, #f4f8ff 70%, #e8f0fb 100%)"
    : `radial-gradient(120% 120% at 28% 22%, rgba(255,255,255,0.55) 0%, rgba(255,255,255,0) 48%), ` +
      `linear-gradient(135deg, rgba(255,255,255,0.30) 0%, rgba(0,0,0,0.22) 100%), ${color}`;

  const textColor = selected ? "#1f2937" : "rgba(255,255,255,0.96)";
  const textShadow = selected ? "none" : "0 1px 2px rgba(0,0,0,0.5)";
  const fontPx = Math.min(12, Math.max(9, h * 0.16));

  return (
    <div
      style={{
        position: "absolute",
        left: node.x ?? 0,
        top: node.y ?? 0,
        width: w,
        height: h,
        backgroundImage: cushion,
        backgroundColor: selected ? "#ffffff" : color,
        borderRadius: 2,
        boxShadow: selected
          ? "0 0 0 1px rgba(34,197,94,0.9), 0 2px 8px rgba(0,0,0,0.25)"
          : "inset 0 1px 1px rgba(255,255,255,0.45), inset 0 -2px 3px rgba(0,0,0,0.22)",
        // Age accent: a thin colored edge (red = old, green = new) along top + left.
        borderTop: `2px solid ${ageColor}`,
        borderLeft: `2px solid ${ageColor}`,
        borderRight: "1px solid rgba(0,0,0,0.16)",
        borderBottom: "1px solid rgba(0,0,0,0.16)",
        overflow: "hidden",
        cursor: isFolder ? "pointer" : "default",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
        padding: "2px 4px",
        opacity: dim ? 0.16 : 1,
        transition: "opacity 0.15s",
        zIndex: selected ? 10 : undefined,
      }}
      onMouseEnter={() => onHoverEnter(node)}
      onMouseLeave={onHoverLeave}
      onClick={isFolder ? handleClick : undefined}
      onContextMenu={(event) => onContext(event, node)}
      title={`${label} — ${formatFileSize(node.data.size)}`}
    >
      {/* Green corner triangles on the selected block */}
      {selected && (
        <>
          <span style={cornerTriangleStyle("tl")} />
          <span style={cornerTriangleStyle("tr")} />
          <span style={cornerTriangleStyle("bl")} />
          <span style={cornerTriangleStyle("br")} />
        </>
      )}
      {showName && (
        <span
          style={{
            fontSize: fontPx,
            fontWeight: 500,
            color: textColor,
            maxWidth: "100%",
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
            lineHeight: 1.25,
            textShadow,
          }}
        >
          {label}
        </span>
      )}
      {showSize && (
        <span
          style={{
            fontSize: Math.max(8, fontPx - 1),
            color: selected ? "#475569" : "rgba(255,255,255,0.88)",
            lineHeight: 1.25,
            textShadow,
          }}
        >
          {formatFileSize(node.data.size)}
        </span>
      )}
    </div>
  );
}

// Small green corner triangle for the selected block (SpaceSniffer marker).
function cornerTriangleStyle(corner: "tl" | "tr" | "bl" | "br"): React.CSSProperties {
  const size = 9;
  const green = "#22c55e";
  const base: React.CSSProperties = {
    position: "absolute",
    width: 0,
    height: 0,
    pointerEvents: "none",
  };
  switch (corner) {
    case "tl":
      return { ...base, top: 0, left: 0, borderTop: `${size}px solid ${green}`, borderRight: `${size}px solid transparent` };
    case "tr":
      return { ...base, top: 0, right: 0, borderTop: `${size}px solid ${green}`, borderLeft: `${size}px solid transparent` };
    case "bl":
      return { ...base, bottom: 0, left: 0, borderBottom: `${size}px solid ${green}`, borderRight: `${size}px solid transparent` };
    case "br":
      return { ...base, bottom: 0, right: 0, borderBottom: `${size}px solid ${green}`, borderLeft: `${size}px solid transparent` };
  }
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
  const [hoveredNode, setHoveredNode] = useState<ComputedNode | null>(null);
  const [renderDepth, setRenderDepth] = useState(3);

  // Filtering state — single SpaceSniffer-style combined query (e.g. "*.mp4;>500Mb").
  const [filterQuery, setFilterQuery] = useState("");
  const [appliedFilter, setAppliedFilter] = useState("");
  const [hideNonMatching, setHideNonMatching] = useState(false);

  // Selected (highlighted) node path — set on hover, SpaceSniffer-style.
  const [selectedPath, setSelectedPath] = useState<string | null>(null);

  // Context menu state
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [actionMessage, setActionMessage] = useState("");

  const unlistenRef = useRef<(() => void) | null>(null);

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

  // Close the context menu on any outside click / escape.
  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setContextMenu(null);
    };
    window.addEventListener("click", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [contextMenu]);

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
    setSelectedPath(null);
    setContextMenu(null);
    setActionMessage("");

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

  // Transform current view for Nivo (nested to renderDepth)
  const nivoData = useMemo(() => {
    if (!currentRoot) return null;
    return transformToNivoData(currentRoot, 0, renderDepth);
  }, [currentRoot, renderDepth]);

  // Age-edge normalization range over the bounded rendered tree.
  const ageRange = useMemo(() => {
    if (!nivoData) return { min: 0, max: 0 };
    return modifiedRange(nivoData);
  }, [nivoData]);

  const filterPredicate = useMemo(() => parseFilterQuery(appliedFilter), [appliedFilter]);
  const filterActive = filterPredicate !== null;

  // Set of node paths that match the active filter (folders included if any descendant matches).
  const matchingPaths = useMemo(() => {
    const set = new Set<string>();
    if (!filterPredicate || !currentRoot) return set;
    const walk = (node: DiskNode): boolean => {
      if (node.nodeType === "folder") {
        let any = false;
        for (const child of node.children) {
          if (walk(child)) any = true;
        }
        if (any) set.add(node.path);
        return any;
      }
      const ok = filterPredicate(node.name, node.size);
      if (ok) set.add(node.path);
      return ok;
    };
    walk(currentRoot);
    return set;
  }, [filterPredicate, currentRoot]);

  const handleZoomIn = useCallback(
    (node: ComputedNode) => {
      if (!currentRoot) return;
      if (node.data.nodeType !== "folder") return;
      const childName = node.data.name;
      const childNode = currentRoot.children.find((c) => c.name === childName && c.nodeType === "folder");
      if (childNode && childNode.children.length > 0) {
        setZoomPath((prev) => [...prev, childName]);
        setHoveredNode(null);
      }
    },
    [currentRoot],
  );

  // Right-click on empty treemap space → zoom out one level (SpaceSniffer-style).
  const handleContainerContextMenu = useCallback(
    (event: React.MouseEvent) => {
      event.preventDefault();
      setZoomPath((prev) => (prev.length > 0 ? prev.slice(0, -1) : prev));
    },
    [],
  );

  const handleNodeContext = useCallback((event: React.MouseEvent, node: ComputedNode) => {
    event.preventDefault();
    event.stopPropagation();
    setContextMenu({ x: event.clientX, y: event.clientY, node });
  }, []);

  // ── File actions ──

  const runAction = useCallback(async (command: string, path: string, fallback: string) => {
    try {
      const result = await invokeTauri<FileActionResult>(command, { path });
      setActionMessage(result.message);
    } catch (e) {
      setError(extractErrorMessage(e, fallback));
    }
  }, []);

  const handleOpen = useCallback(
    (node: ComputedNode) => {
      setContextMenu(null);
      void runAction("open_path", node.data.path, "打开失败");
    },
    [runAction],
  );

  const handleReveal = useCallback(
    (node: ComputedNode) => {
      setContextMenu(null);
      void runAction("reveal_in_file_manager", node.data.path, "在文件管理器中显示失败");
    },
    [runAction],
  );

  const handleCopyPath = useCallback(async (node: ComputedNode) => {
    setContextMenu(null);
    try {
      await navigator.clipboard.writeText(node.data.path);
      setActionMessage(`已复制路径: ${node.data.path}`);
    } catch {
      setError("复制路径失败");
    }
  }, []);

  const handleDelete = useCallback(async (node: ComputedNode) => {
    setContextMenu(null);
    const target = node.data;
    const isFolder = target.nodeType === "folder";
    const confirmed = window.confirm(
      `确定要删除${isFolder ? "文件夹（含全部内容）" : "文件"}吗？\n${target.path}\n\n该项目将被移至回收站。`,
    );
    if (!confirmed) return;
    try {
      const result = await invokeTauri<FileActionResult>("delete_path", { path: target.path });
      setActionMessage(result.message);
      // Local tree mutation + recompute totals (no rescan).
      setScanResult((prev) => {
        if (!prev) return prev;
        const { node: newRoot, changed } = removeNodeByPath(prev.root, target.path);
        if (!changed) return prev;
        const { files, dirs } = countItems(newRoot);
        return {
          ...prev,
          root: newRoot,
          totalSize: newRoot.size,
          totalFiles: files,
          totalDirs: dirs,
        };
      });
      setHoveredNode(null);
    } catch (e) {
      setError(extractErrorMessage(e, "删除失败"));
    }
  }, []);

  // ── Filtering ──

  const applyFilter = useCallback(() => {
    setAppliedFilter(filterQuery);
  }, [filterQuery]);

  const clearFilters = useCallback(() => {
    setFilterQuery("");
    setAppliedFilter("");
    setHideNonMatching(false);
  }, []);

  // Collect categories present in the bounded rendered tree for the legend.
  const presentCategories = useMemo(() => {
    if (!nivoData) return new Set<string>();
    const categories = new Set<string>();
    const walk = (node: TreemapDatum) => {
      if (node.nodeType === "folder") {
        categories.add("folder");
      } else {
        categories.add(getFileCategory(node.nodeType));
      }
      node.children?.forEach(walk);
    };
    walk(nivoData);
    return categories;
  }, [nivoData]);

  return (
    <div className="space-y-5">
      {/* Header */}
      <div>
        <h2 className="text-xl font-semibold text-slate-800">磁盘分析热力图</h2>
        <p className="text-sm text-slate-500 mt-1">
          参考 SpaceSniffer 的立体方块热力图：嵌套展示磁盘占用，单击文件夹钻入、右键空白处返回上级，支持筛选与右键文件操作。
        </p>
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
          <label className="flex items-center gap-2 text-sm text-slate-600">
            <span className="font-medium">嵌套层级:</span>
            <input
              type="range"
              min={1}
              max={8}
              value={renderDepth}
              onChange={(e) => setRenderDepth(Number(e.target.value))}
              className="w-24 accent-emerald-500"
            />
            <span className="text-xs text-slate-400 w-4 text-right">{renderDepth}</span>
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

      {/* Filter bar — single combined SpaceSniffer-style query */}
      {scanResult && (
        <div className="bg-white rounded-xl border border-slate-200 p-4 shadow-sm">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="text-sm font-medium text-slate-600">Filter</span>
            <input
              type="text"
              value={filterQuery}
              onChange={(e) => setFilterQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") applyFilter();
              }}
              placeholder="*.mp4;>500Mb"
              className="flex-1 min-w-[200px] px-3 py-1.5 rounded-lg border border-slate-300 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-blue-400"
            />
            <button
              type="button"
              onClick={applyFilter}
              className="px-4 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-sm font-medium transition-colors"
            >
              Filter
            </button>
            <label className="flex items-center gap-1.5 text-sm text-slate-600 cursor-pointer">
              <input
                type="checkbox"
                checked={hideNonMatching}
                onChange={(e) => setHideNonMatching(e.target.checked)}
                className="accent-blue-500"
              />
              <span>隐藏不匹配</span>
            </label>
            <button
              type="button"
              onClick={clearFilters}
              className="px-3 py-1.5 rounded-lg bg-slate-100 hover:bg-slate-200 text-xs font-medium text-slate-600 transition-colors"
            >
              清除
            </button>
          </div>
          <p className="mt-2 text-xs text-slate-400">
            语法：名称通配 <code className="font-mono">*.mp4</code>、大小比较{" "}
            <code className="font-mono">&gt;500Mb</code> / <code className="font-mono">&lt;1gb</code>，多个条件用{" "}
            <code className="font-mono">;</code> 分隔（同时满足）。
          </p>
        </div>
      )}

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

      {/* Error / action message */}
      {error && (
        <div className="bg-red-50 border border-red-200 rounded-lg px-4 py-3 text-sm text-red-700">{error}</div>
      )}
      {actionMessage && !error && (
        <div className="bg-emerald-50 border border-emerald-200 rounded-lg px-4 py-2 text-sm text-emerald-700">
          {actionMessage}
        </div>
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
                  onClick={() => setZoomPath(zoomPath.slice(0, i + 1))}
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

      {/* Folder header strip (SpaceSniffer-style) + treemap container */}
      {nivoData && currentRoot && (
        <div
          className="flex items-center justify-between rounded-t-xl px-4 py-2 text-sm font-medium"
          style={{
            background: "linear-gradient(180deg, #f5deb8 0%, #e9c795 100%)",
            color: "#7a5a2a",
            border: "1px solid #d8b67e",
            borderBottom: "none",
            marginBottom: "-12px",
          }}
        >
          <span className="truncate">{currentRoot.name}</span>
          <span>{formatFileSize(currentRoot.size)}</span>
        </div>
      )}

      {/* Treemap container — left-click drills in, right-click zooms out */}
      <div
        className="bg-slate-900 rounded-xl overflow-hidden shadow-lg relative group"
        style={{ height: nivoData && currentRoot ? 500 : 0, transition: "height 0.2s ease" }}
        onContextMenu={handleContainerContextMenu}
      >
        {nivoData && currentRoot && (
          <div style={{ width: "100%", height: "100%" }}>
            <ResponsiveTreeMapHtml
              data={nivoData}
              identity="id"
              value="value"
              tile="squarify"
              margin={{ top: 2, right: 2, bottom: 2, left: 2 }}
              colors={() => ""}
              nodeComponent={(props) => {
                const node = (props as unknown as { node: ComputedNode }).node;
                // Don't render a wrapper block for the view root — show its
                // children directly (matching SpaceSniffer's "only children of
                // the current folder" behaviour). The folder header strip already
                // labels the current location.
                if (node.data.id === currentRoot.path) {
                  return <div style={{ display: "none" }} />;
                }
                // Also suppress the synthetic "其它 (N 项)" leaf when it has
                // no area assigned (Nivo assigns value 0 → won't render).
                const isMatch = matchingPaths.has(node.data.path);
                if (filterActive && hideNonMatching && !isMatch) {
                  return <div style={{ display: "none" }} />;
                }
                return (
                  <TreemapNode
                    node={node}
                    dim={filterActive && !isMatch}
                    selected={selectedPath === node.data.path}
                    ageColor={getAgeEdgeColor(node.data.modifiedSecs, ageRange.min, ageRange.max)}
                    onZoomIn={handleZoomIn}
                    onContext={handleNodeContext}
                    onHoverEnter={(n) => {
                      setHoveredNode(n);
                      setSelectedPath(n.data.path);
                    }}
                    onHoverLeave={() => setHoveredNode(null)}
                  />
                );
              }}
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
              单击钻入 · 右键返回
            </span>
          </div>
        )}
      </div>

      {/* Context menu */}
      {contextMenu && (
        <div
          className="fixed z-50 min-w-[160px] rounded-lg border border-slate-200 bg-white py-1 shadow-xl text-sm"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          <div className="px-3 py-1.5 text-xs text-slate-400 truncate max-w-[240px]">
            {contextMenu.node.data.name}
          </div>
          <button
            type="button"
            onClick={() => handleOpen(contextMenu.node)}
            className="block w-full px-3 py-1.5 text-left text-slate-700 hover:bg-slate-100"
          >
            打开
          </button>
          <button
            type="button"
            onClick={() => handleReveal(contextMenu.node)}
            className="block w-full px-3 py-1.5 text-left text-slate-700 hover:bg-slate-100"
          >
            在文件管理器中显示
          </button>
          <button
            type="button"
            onClick={() => handleCopyPath(contextMenu.node)}
            className="block w-full px-3 py-1.5 text-left text-slate-700 hover:bg-slate-100"
          >
            复制路径
          </button>
          <div className="my-1 border-t border-slate-100" />
          <button
            type="button"
            onClick={() => handleDelete(contextMenu.node)}
            className="block w-full px-3 py-1.5 text-left text-red-600 hover:bg-red-50"
          >
            删除（移至回收站）
          </button>
        </div>
      )}

      {scanResult && (
        <>
          {/* Hovered node detail */}
          {hoveredNode && (
            <div className="bg-white rounded-lg border border-slate-200 px-4 py-3 shadow-sm text-sm flex items-center gap-4">
              <span className="font-medium text-slate-700">{hoveredNode.data.name}</span>
              <span className="text-slate-500">
                {hoveredNode.data.nodeType === "folder"
                  ? `文件夹 — ${formatFileSize(hoveredNode.data.size)}`
                  : `${hoveredNode.data.nodeType.toUpperCase()} — ${formatFileSize(hoveredNode.data.size)}`}
              </span>
              <span
                className="w-3 h-3 rounded-sm inline-block"
                style={{ background: getCategoryColor(hoveredNode.data.nodeType) }}
              />
            </div>
          )}

          {/* Legend */}
          <div className="flex flex-wrap gap-3 text-xs text-slate-500">
            {Object.entries(CATEGORY_LABELS)
              .filter(([key]) => presentCategories.has(key))
              .map(([key, label]) => (
                <span key={key} className="flex items-center gap-1.5">
                  <span className="w-3 h-3 rounded-sm inline-block" style={{ background: CATEGORY_COLORS[key] }} />
                  {label}
                </span>
              ))}
          </div>
        </>
      )}
    </div>
  );
}
