export type ToolItem = {
  href: string;
  title: string;
  shortLabel: string;
  description: string;
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
];

const RECENT_TOOLS_KEY = "devtoolkit.recent-tools";
const MAX_RECENT_TOOLS = 5;

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
  const isKnownTool = TOOL_ITEMS.some((item) => item.href === href);
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
