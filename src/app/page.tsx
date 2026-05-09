"use client";

import Link from "next/link";
import RecentTools from "@/components/recent-tools";
import { initPlatformDetection, useVisibleTools } from "@/lib/tools";

initPlatformDetection();

export default function Home() {
  const visibleTools = useVisibleTools();

  return (
    <main className="mx-auto flex min-h-screen w-full max-w-5xl flex-col px-4 py-10 sm:px-8">
      <div className="rounded-3xl border border-[var(--card-border)] bg-[var(--card)] p-8 shadow-[0_24px_65px_-35px_rgba(17,97,125,0.55)] backdrop-blur-xl">
        <p className="font-mono text-xs uppercase tracking-[0.2em] text-[var(--accent)]">DevToolkit Desktop</p>
        <h1 className="mt-3 text-4xl font-semibold tracking-tight">开发工具导航</h1>
        <p className="mt-3 max-w-2xl text-sm leading-7 text-slate-700">
          工具已拆分为独立页面，避免所有功能堆在同一个视图中。你可以按功能进入对应路由。
        </p>

        <div className="mt-7 grid grid-cols-1 gap-4 sm:grid-cols-2">
          {visibleTools.map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className="rounded-2xl border border-slate-200 bg-white/75 p-5 transition hover:-translate-y-0.5 hover:shadow-lg"
            >
              <h2 className="text-lg font-semibold">{item.title}</h2>
              <p className="mt-2 text-sm text-slate-600">{item.description}</p>
            </Link>
          ))}
        </div>

        <div className="mt-5">
          <RecentTools />
        </div>
      </div>
    </main>
  );
}
