"use client";

import { useSyncExternalStore } from "react";

let cachedIsWindows: boolean | null = null;

export async function initPlatformDetection(): Promise<void> {
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    try {
      const { platform } = await import("@tauri-apps/plugin-os");
      const os = await platform();
      cachedIsWindows = os === "windows";
    } catch {
      cachedIsWindows = detectWindowsViaBrowser();
    }
  } else {
    cachedIsWindows = detectWindowsViaBrowser();
  }
}

function detectWindowsViaBrowser(): boolean {
  if (typeof window === "undefined") return false;
  const source = `${window.navigator.platform} ${window.navigator.userAgent}`.toLowerCase();
  return source.includes("win");
}

export type ToolItem = {
  href: string;
  title: string;
  shortLabel: string;
  description: string;
  windowsOnly?: boolean;
};

export const TOOL_ITEMS: ToolItem[] = [
  {
    href: "/tools/archive",
    title: "压缩 / 解压工具",
    shortLabel: "压缩/解压",
    description: "支持 zip、tar、tar.gz、tar.bz2、tar.xz 和过滤规则。",
  },
  {
    href: "/tools/system",
    title: "系统与目录工具",
    shortLabel: "系统/目录",
    description: "读取系统信息、扫描目录，便于调试 Tauri 命令。",
  },
  {
    href: "/tools/windows-migrate",
    title: "Windows 程序迁移工具",
    shortLabel: "程序迁移",
    description: "迁移已安装程序到新目录，并同步注册表、快捷方式、环境变量。",
    windowsOnly: true,
  },
  {
    href: "/tools/tool-data-migrate",
    title: "工具数据迁移",
    shortLabel: "数据迁移",
    description: "将如 .claude 等工具数据目录迁移到其它盘，支持软链接或环境变量。",
    windowsOnly: true,
  },
  {
    href: "/tools/disk-heatmap",
    title: "磁盘分析热力图",
    shortLabel: "磁盘热力图",
    description: "可视化磁盘使用分布，按文件类型着色，支持钻入查看子目录。",
  },
];

const RECENT_TOOLS_KEY = "devtoolkit.recent-tools";
const MAX_RECENT_TOOLS = 5;

export function isWindowsPlatform(): boolean {
  if (cachedIsWindows !== null) {
    return cachedIsWindows;
  }
  return detectWindowsViaBrowser();
}

export function getVisibleTools(): ToolItem[] {
  const windows = isWindowsPlatform();
  return TOOL_ITEMS.filter((item) => (item.windowsOnly ? windows : true));
}

export function getDefaultVisibleTools(): ToolItem[] {
  return TOOL_ITEMS.filter((item) => !item.windowsOnly);
}

export function getRecentToolHrefs(): string[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(RECENT_TOOLS_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const valid = parsed.filter((item): item is string => typeof item === "string");
    return valid.slice(0, MAX_RECENT_TOOLS);
  } catch {
    return [];
  }
}

export function pushRecentTool(href: string): string[] {
  if (typeof window === "undefined") return [];
  const visible = getVisibleTools();
  const isKnownTool = visible.some((item) => item.href === href);
  if (!isKnownTool) return getRecentToolHrefs();

  const current = getRecentToolHrefs().filter((item) => item !== href);
  const next = [href, ...current].slice(0, MAX_RECENT_TOOLS);
  try {
    window.localStorage.setItem(RECENT_TOOLS_KEY, JSON.stringify(next));
    window.dispatchEvent(new Event("devtoolkit:recent-tools-changed"));
  } catch {
    return current;
  }
  return next;
}

export function findToolByHref(href: string) {
  return TOOL_ITEMS.find((item) => item.href === href);
}

function subscribeNoop(): () => void {
  return () => {};
}

export function useIsWindowsPlatform(): boolean {
  return useSyncExternalStore(subscribeNoop, isWindowsPlatform, () => false);
}

export function useVisibleTools(): ToolItem[] {
  const windows = useIsWindowsPlatform();
  return TOOL_ITEMS.filter((item) => (item.windowsOnly ? windows : true));
}
