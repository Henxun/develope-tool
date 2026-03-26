"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { findToolByHref, getRecentToolHrefs } from "@/lib/tools";

type RecentToolsProps = {
  title?: string;
  emptyText?: string;
};

export default function RecentTools({
  title = "最近使用",
  emptyText = "暂无最近使用记录",
}: RecentToolsProps) {
  const [recentHrefs, setRecentHrefs] = useState<string[]>([]);

  useEffect(() => {
    const sync = () => setRecentHrefs(getRecentToolHrefs());
    sync();
    window.addEventListener("storage", sync);
    window.addEventListener("devtoolkit:recent-tools-changed", sync);
    return () => {
      window.removeEventListener("storage", sync);
      window.removeEventListener("devtoolkit:recent-tools-changed", sync);
    };
  }, []);

  const recent = recentHrefs.map((href) => findToolByHref(href)).filter((item) => item !== undefined);

  return (
    <div className="rounded-2xl border border-slate-200 bg-white/75 p-4">
      <p className="text-sm font-semibold">{title}</p>
      {recent.length ? (
        <div className="mt-3 flex flex-wrap gap-2 text-xs">
          {recent.map((tool) => (
            <Link
              key={tool.href}
              href={tool.href}
              className="rounded-lg border border-slate-300 bg-white px-2.5 py-1.5 transition hover:bg-slate-100"
            >
              {tool.shortLabel}
            </Link>
          ))}
        </div>
      ) : (
        <p className="mt-2 text-xs text-slate-500">{emptyText}</p>
      )}
    </div>
  );
}
