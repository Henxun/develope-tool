"use client";

import { FormEvent, useMemo, useState } from "react";
import { invokeTauri } from "@/lib/tauri";

type SystemInfo = {
  os: string;
  arch: string;
  currentDir: string;
  nodeEnv: string;
};

type FileEntry = {
  name: string;
  path: string;
  isDir: boolean;
};

export default function SystemToolPage() {
  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null);
  const [dirInput, setDirInput] = useState("");
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [error, setError] = useState("");
  const [loadingInfo, setLoadingInfo] = useState(false);
  const [loadingFiles, setLoadingFiles] = useState(false);

  const currentPathLabel = useMemo(() => {
    if (!entries.length) return "尚未读取";
    return entries[0]?.path ? entries[0].path.replace(/[^\\/]+$/, "") : "未知";
  }, [entries]);

  const handleGetSystemInfo = async () => {
    setError("");
    setLoadingInfo(true);
    try {
      const result = await invokeTauri<SystemInfo>("get_system_info");
      setSystemInfo(result);
    } catch (err) {
      setError(err instanceof Error ? err.message : "读取系统信息失败");
    } finally {
      setLoadingInfo(false);
    }
  };

  const handleScanDirectory = async (event?: FormEvent) => {
    event?.preventDefault();
    setError("");
    setLoadingFiles(true);
    try {
      const result = await invokeTauri<FileEntry[]>("list_directory", {
        path: dirInput.trim() ? dirInput.trim() : null,
      });
      setEntries(result);
    } catch (err) {
      setError(err instanceof Error ? err.message : "目录读取失败");
      setEntries([]);
    } finally {
      setLoadingFiles(false);
    }
  };

  return (
    <main className="rounded-3xl border border-[var(--card-border)] bg-[var(--card)] p-6 shadow-[0_24px_65px_-35px_rgba(17,97,125,0.55)] backdrop-blur-xl sm:p-8">
      <h1 className="text-3xl font-semibold tracking-tight">系统与目录工具</h1>
      <p className="mt-3 text-sm leading-7 text-slate-700">读取系统信息、查看目录内容，用于开发与调试。</p>

      <div className="mt-6 space-y-3">
        <button
          type="button"
          onClick={handleGetSystemInfo}
          className="inline-flex h-10 items-center rounded-xl bg-[var(--accent)] px-4 text-sm font-medium text-white transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
          disabled={loadingInfo}
        >
          {loadingInfo ? "读取中..." : "获取系统信息"}
        </button>

        {systemInfo ? (
          <div className="grid grid-cols-2 gap-3 rounded-2xl border border-slate-200 bg-white/70 p-4 text-xs sm:grid-cols-4">
            <div>
              <p className="text-slate-500">系统</p>
              <p className="mt-1 font-medium">{systemInfo.os}</p>
            </div>
            <div>
              <p className="text-slate-500">架构</p>
              <p className="mt-1 font-medium">{systemInfo.arch}</p>
            </div>
            <div>
              <p className="text-slate-500">环境</p>
              <p className="mt-1 font-medium">{systemInfo.nodeEnv}</p>
            </div>
            <div>
              <p className="text-slate-500">工作目录</p>
              <p className="mt-1 truncate font-medium" title={systemInfo.currentDir}>
                {systemInfo.currentDir}
              </p>
            </div>
          </div>
        ) : null}
      </div>

      <form onSubmit={handleScanDirectory} className="mt-5 flex flex-col gap-3 sm:flex-row">
        <input
          value={dirInput}
          onChange={(event) => setDirInput(event.target.value)}
          className="h-10 flex-1 rounded-xl border border-slate-300 bg-white px-4 text-sm outline-none ring-[var(--accent)]/40 transition focus:ring"
          placeholder="例如：E:\\workspace"
        />
        <button
          type="submit"
          className="h-10 rounded-xl border border-slate-300 px-4 text-sm font-medium transition hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-60"
          disabled={loadingFiles}
        >
          {loadingFiles ? "读取中..." : "扫描目录"}
        </button>
      </form>

      <p className="mt-4 text-xs text-slate-500">当前路径：{currentPathLabel}</p>
      <div className="mt-3 h-96 overflow-auto rounded-2xl border border-slate-200 bg-slate-50/80 p-2">
        {entries.length ? (
          <ul className="space-y-1">
            {entries.map((entry) => (
              <li key={entry.path} className="flex items-center justify-between rounded-xl px-3 py-2 text-sm hover:bg-white">
                <span className="truncate pr-2">{entry.name}</span>
                <span className="shrink-0 font-mono text-xs text-slate-500">{entry.isDir ? "DIR" : "FILE"}</span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="p-3 text-sm text-slate-500">暂无数据，先执行一次目录扫描。</p>
        )}
      </div>

      {error ? <p className="mt-5 rounded-xl border border-rose-300 bg-rose-50 px-4 py-3 text-sm text-rose-700">{error}</p> : null}
    </main>
  );
}
