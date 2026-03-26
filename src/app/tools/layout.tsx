"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect } from "react";
import RecentTools from "@/components/recent-tools";
import { TOOL_ITEMS, pushRecentTool } from "@/lib/tools";

function navButtonClass(active: boolean): string {
  if (active) {
    return "rounded-lg border border-[var(--accent)] bg-[var(--accent)] px-3 py-1.5 text-white transition";
  }
  return "rounded-lg border border-slate-300 bg-white/70 px-3 py-1.5 transition hover:bg-white";
}

export default function ToolsLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  const pathname = usePathname();

  useEffect(() => {
    pushRecentTool(pathname);
  }, [pathname]);

  return (
    <div className="mx-auto flex min-h-screen w-full max-w-7xl flex-col px-4 py-8 sm:px-8 sm:py-10">
      <header className="mb-5 rounded-2xl border border-[var(--card-border)] bg-[var(--card)] p-4 backdrop-blur-xl">
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <Link href="/" className={navButtonClass(pathname === "/")}>
            主页
          </Link>
          <Link href="/tools" className={navButtonClass(pathname === "/tools")}>
            工具列表
          </Link>
          {TOOL_ITEMS.map((item) => (
            <Link key={item.href} href={item.href} className={navButtonClass(pathname === item.href)}>
              {item.shortLabel}
            </Link>
          ))}
        </div>
        <div className="mt-3">
          <RecentTools title="最近使用工具" emptyText="还没有访问过工具页面" />
        </div>
      </header>
      {children}
    </div>
  );
}
