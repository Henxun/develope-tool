"use client";

import Link from "next/link";
import { useVisibleTools } from "@/lib/tools";

export default function ToolsIndexPage() {
  const visibleTools = useVisibleTools();

  return (
    <main className="grid flex-1 grid-cols-1 gap-4 sm:grid-cols-2">
      {visibleTools.map((item) => (
        <Link key={item.href} href={item.href} className="rounded-2xl border border-slate-200 bg-white/75 p-6 transition hover:shadow-lg">
          <h1 className="text-xl font-semibold">{item.title}</h1>
          <p className="mt-2 text-sm text-slate-600">{item.description}</p>
        </Link>
      ))}
    </main>
  );
}
