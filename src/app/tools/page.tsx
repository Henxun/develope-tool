import Link from "next/link";
import { TOOL_ITEMS } from "@/lib/tools";

export default function ToolsIndexPage() {
  return (
    <main className="grid flex-1 grid-cols-1 gap-4 sm:grid-cols-2">
      {TOOL_ITEMS.map((item) => (
        <Link key={item.href} href={item.href} className="rounded-2xl border border-slate-200 bg-white/75 p-6 transition hover:shadow-lg">
          <h1 className="text-xl font-semibold">{item.title}</h1>
          <p className="mt-2 text-sm text-slate-600">{item.description}</p>
        </Link>
      ))}
    </main>
  );
}
