"use client";

import { FormEvent, useEffect, useMemo, useState } from "react";
import { useIsWindowsPlatform } from "@/lib/tools";
import { invokeTauri, toSinglePath } from "@/lib/tauri";

type ToolDataMigrationResult = {
  toolName: string;
  sourceDir: string;
  targetDir: string;
  strategy: string;
  moved: boolean;
  symlinkCreated: boolean;
  envVarUpdated: boolean;
  warnings: string[];
};

type MigrationLogEvent = {
  level: string;
  message: string;
  timestamp: string;
};

type QuickPreset = {
  label: string;
  toolName: string;
  folderName: string;
  strategy: "symlink" | "env" | "both";
  envVarName?: string;
};

const QUICK_PRESETS: QuickPreset[] = [
  { label: ".claude", toolName: "Claude", folderName: ".claude", strategy: "both", envVarName: "CLAUDE_CONFIG_DIR" },
  { label: ".dotnet", toolName: "dotnet", folderName: ".dotnet", strategy: "symlink" },
  { label: ".nuget", toolName: "nuget", folderName: ".nuget", strategy: "symlink" },
  { label: ".opencode", toolName: "opencode", folderName: ".opencode", strategy: "both", envVarName: "OPENCODE_HOME" },
  { label: ".rustup", toolName: "rustup", folderName: ".rustup", strategy: "both", envVarName: "RUSTUP_HOME" },
  { label: ".trae-cn", toolName: "trae-cn", folderName: ".trae-cn", strategy: "symlink" },
];

function extractErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (typeof error === "object" && error !== null && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return fallback;
}

function buildPresetSource(folderName: string): string {
  return `%USERPROFILE%\\${folderName}`;
}

function buildPresetTarget(targetRoot: string, folderName: string): string {
  return `${targetRoot.replace(/[\\/]+$/, "")}\\${folderName}`;
}

export default function ToolDataMigratePage() {
  const isWindows = useIsWindowsPlatform();
  const [toolName, setToolName] = useState("Claude");
  const [sourceDir, setSourceDir] = useState("%USERPROFILE%\\.claude");
  const [targetDir, setTargetDir] = useState("D:\\tool-data\\.claude");
  const [strategy, setStrategy] = useState("both");
  const [envVarName, setEnvVarName] = useState("CLAUDE_CONFIG_DIR");
  const [targetRoot, setTargetRoot] = useState("D:\\tool-data");
  const [dryRun, setDryRun] = useState(true);
  const [loading, setLoading] = useState(false);
  const [quickLoadingLabel, setQuickLoadingLabel] = useState("");
  const [loadingTick, setLoadingTick] = useState(0);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ToolDataMigrationResult | null>(null);
  const [taskLogs, setTaskLogs] = useState<string[]>([]);

  const isBusy = loading || quickLoadingLabel.length > 0;

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setup = async () => {
      if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
      const { listen } = await import("@tauri-apps/api/event");
      unlisten = await listen<MigrationLogEvent>("tool-data-migrate-log", (event) => {
        const payload = event.payload;
        const stamp = Number(payload.timestamp);
        const timeLabel = Number.isFinite(stamp)
          ? new Date(stamp * 1000).toLocaleTimeString("zh-CN", { hour12: false })
          : "--:--:--";
        const line = `[${timeLabel}] ${payload.level.toUpperCase()} ${payload.message}`;
        setTaskLogs((current) => [...current, line].slice(-120));
      });
    };

    void setup();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  useEffect(() => {
    if (!isBusy) {
      setLoadingTick(0);
      return;
    }

    const timer = window.setInterval(() => {
      setLoadingTick((current) => current + 1);
    }, 350);

    return () => {
      window.clearInterval(timer);
    };
  }, [isBusy]);

  const loadingDots = ".".repeat((loadingTick % 3) + 1);
  const elapsedSeconds = Math.floor((loadingTick * 350) / 1000);
  const hasLogs = useMemo(() => taskLogs.length > 0, [taskLogs.length]);

  const executeMigration = async (payload: {
    toolName: string;
    sourceDir: string;
    targetDir: string;
    strategy: string;
    envVarName: string | null;
    dryRun: boolean;
  }) => {
    const data = await invokeTauri<ToolDataMigrationResult>("migrate_tool_data", {
      request: payload,
    });
    setResult(data);
  };

  const pickSourceDir = async () => {
    setError("");
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = toSinglePath(await open({ directory: true, multiple: false, title: "选择原始数据目录" }));
      if (selected) setSourceDir(selected);
    } catch (err) {
      setError(extractErrorMessage(err, "选择目录失败"));
    }
  };

  const pickTargetParent = async () => {
    setError("");
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = toSinglePath(await open({ directory: true, multiple: false, title: "选择目标父目录" }));
      if (!selected) return;
      const leaf = sourceDir.split(/[\\/]/).filter(Boolean).at(-1) || "tool-data";
      setTargetDir(`${selected.replace(/[\\/]+$/, "")}\\${leaf}`);
    } catch (err) {
      setError(extractErrorMessage(err, "选择目录失败"));
    }
  };

  const pickTargetRoot = async () => {
    setError("");
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = toSinglePath(await open({ directory: true, multiple: false, title: "选择一键迁移目标根目录" }));
      if (selected) setTargetRoot(selected);
    } catch (err) {
      setError(extractErrorMessage(err, "选择目录失败"));
    }
  };

  const applyPreset = (preset: QuickPreset) => {
    setToolName(preset.toolName);
    setSourceDir(buildPresetSource(preset.folderName));
    setTargetDir(buildPresetTarget(targetRoot, preset.folderName));
    setStrategy(preset.strategy);
    setEnvVarName(preset.envVarName ?? "");
  };

  const runQuickMigration = async (preset: QuickPreset) => {
    setError("");
    setResult(null);
    setTaskLogs([]);
    setQuickLoadingLabel(preset.label);

    try {
      await executeMigration({
        toolName: preset.toolName,
        sourceDir: buildPresetSource(preset.folderName),
        targetDir: buildPresetTarget(targetRoot, preset.folderName),
        strategy: preset.strategy,
        envVarName: preset.envVarName ?? null,
        dryRun,
      });
      applyPreset(preset);
    } catch (err) {
      setError(extractErrorMessage(err, `一键迁移 ${preset.label} 失败`));
    } finally {
      setQuickLoadingLabel("");
    }
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setError("");
    setResult(null);
    setTaskLogs([]);
    if (!isWindows) {
      setError("该工具仅支持 Windows 平台");
      return;
    }

    if (!toolName.trim() || !sourceDir.trim() || !targetDir.trim()) {
      setError("请填写工具名称、源目录、目标目录");
      return;
    }

    setLoading(true);
    try {
      await executeMigration({
        toolName: toolName.trim(),
        sourceDir: sourceDir.trim(),
        targetDir: targetDir.trim(),
        strategy,
        envVarName: envVarName.trim() || null,
        dryRun,
      });
    } catch (err) {
      setError(extractErrorMessage(err, "迁移失败"));
    } finally {
      setLoading(false);
    }
  };

  if (!isWindows) {
    return (
      <main className="rounded-3xl border border-[var(--card-border)] bg-[var(--card)] p-8 shadow-[0_24px_65px_-35px_rgba(17,97,125,0.55)] backdrop-blur-xl">
        <h1 className="text-3xl font-semibold tracking-tight">工具数据迁移</h1>
        <p className="mt-4 text-sm text-slate-700">当前环境不是 Windows，该工具仅在 Windows 桌面端显示。</p>
      </main>
    );
  }

  return (
    <main className="rounded-3xl border border-[var(--card-border)] bg-[var(--card)] p-6 shadow-[0_24px_65px_-35px_rgba(17,97,125,0.55)] backdrop-blur-xl sm:p-8">
      <h1 className="text-3xl font-semibold tracking-tight">工具数据迁移</h1>
      <p className="mt-3 text-sm text-slate-700">支持 `.dotnet`、`.nuget`、`.opencode`、`.rustup`、`.trae-cn` 等目录一键迁移。</p>

      <section className="mt-4 rounded-2xl border border-slate-200 bg-white/75 p-4">
        <h2 className="text-sm font-semibold">一键迁移快捷按钮</h2>
        <div className="mt-3 flex flex-col gap-2 sm:flex-row sm:items-center">
          <input
            value={targetRoot}
            onChange={(event) => setTargetRoot(event.target.value)}
            className="h-10 flex-1 rounded-xl border border-slate-300 px-3 text-sm"
            placeholder="目标根目录（例如 D:\\tool-data）"
          />
          <button type="button" onClick={pickTargetRoot} className="h-10 rounded-xl border border-slate-300 px-3 text-sm font-medium hover:bg-slate-100">
            选择目标根目录
          </button>
        </div>
        <div className="mt-3 flex flex-wrap gap-2">
          {QUICK_PRESETS.map((preset) => (
            <button
              key={preset.label}
              type="button"
              onClick={() => void runQuickMigration(preset)}
              disabled={isBusy}
              className="h-9 rounded-lg border border-slate-300 bg-white px-3 text-xs font-medium transition hover:bg-slate-100 disabled:opacity-60"
            >
              {quickLoadingLabel === preset.label ? `${preset.label} 迁移中...` : `一键迁移 ${preset.label}`}
            </button>
          ))}
        </div>
      </section>

      <form onSubmit={handleSubmit} className="mt-4 rounded-2xl border border-slate-200 bg-white/80 p-4">
        <div className="grid grid-cols-1 gap-3">
          <input value={toolName} onChange={(e) => setToolName(e.target.value)} className="h-10 rounded-xl border border-slate-300 px-3 text-sm" placeholder="工具名称" />
          <input value={sourceDir} onChange={(e) => setSourceDir(e.target.value)} className="h-10 rounded-xl border border-slate-300 px-3 text-sm" placeholder="源目录（可用 %USERPROFILE%）" />
          <button type="button" onClick={pickSourceDir} className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium hover:bg-slate-100">选择源目录</button>
          <input value={targetDir} onChange={(e) => setTargetDir(e.target.value)} className="h-10 rounded-xl border border-slate-300 px-3 text-sm" placeholder="目标目录" />
          <button type="button" onClick={pickTargetParent} className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium hover:bg-slate-100">选择目标父目录</button>

          <select value={strategy} onChange={(e) => setStrategy(e.target.value)} className="h-10 rounded-xl border border-slate-300 px-3 text-sm">
            <option value="symlink">仅软链接</option>
            <option value="env">仅环境变量</option>
            <option value="both">软链接 + 环境变量</option>
          </select>

          <input value={envVarName} onChange={(e) => setEnvVarName(e.target.value)} className="h-10 rounded-xl border border-slate-300 px-3 text-sm" placeholder="环境变量名（例如 CLAUDE_CONFIG_DIR）" />

          <label className="flex items-center gap-2 text-sm text-slate-700">
            <input type="checkbox" checked={dryRun} onChange={(e) => setDryRun(e.target.checked)} />
            仅模拟（dry-run）
          </label>

          <button type="submit" disabled={isBusy} className="h-10 rounded-xl bg-[var(--accent)] px-4 text-sm font-medium text-white disabled:opacity-60">
            {loading ? "迁移中..." : dryRun ? "开始模拟迁移" : "开始迁移"}
          </button>
        </div>
      </form>

      {isBusy ? (
        <div className="mt-4 rounded-2xl border border-sky-200 bg-sky-50 p-4 text-sm text-sky-800">
          <div className="flex items-center gap-3">
            <span className="inline-block h-4 w-4 animate-spin rounded-full border-2 border-sky-300 border-t-sky-700" />
            <p className="font-medium">迁移任务执行中{loadingDots}</p>
          </div>
          <p className="mt-2 text-xs text-sky-700">已运行约 {elapsedSeconds} 秒，UI 仍可响应，请耐心等待任务完成。</p>
        </div>
      ) : null}

      <section className="mt-4 rounded-2xl border border-slate-200 bg-white/80 p-4">
        <div className="mb-2 flex items-center justify-between">
          <h2 className="text-sm font-semibold">后台任务日志流</h2>
          <button
            type="button"
            onClick={() => setTaskLogs([])}
            className="h-7 rounded-lg border border-slate-300 px-2 text-xs font-medium hover:bg-slate-100"
          >
            清空
          </button>
        </div>
        <div className="h-44 overflow-auto rounded-xl border border-slate-200 bg-slate-950 p-3 font-mono text-[11px] leading-5 text-slate-100">
          {hasLogs ? (
            <ul className="space-y-1">
              {taskLogs.map((line, index) => (
                <li key={`${index}-${line}`}>{line}</li>
              ))}
            </ul>
          ) : (
            <p className="text-slate-400">暂无日志，执行迁移后会实时显示步骤信息。</p>
          )}
        </div>
      </section>

      {result ? (
        <div className="mt-4 rounded-2xl border border-emerald-200 bg-emerald-50 p-4 text-sm text-emerald-800">
          <p>工具：{result.toolName}</p>
          <p className="mt-1">目录：{result.sourceDir} -&gt; {result.targetDir}</p>
          <p className="mt-1">策略：{result.strategy}</p>
          <p className="mt-1">移动目录：{result.moved ? "是" : "否（dry-run）"}</p>
          <p className="mt-1">软链接：{result.symlinkCreated ? "已创建" : "未创建"}</p>
          <p className="mt-1">环境变量：{result.envVarUpdated ? "已更新" : "未更新"}</p>
          {result.warnings.length ? (
            <ul className="mt-2 list-disc space-y-1 pl-5 text-xs text-amber-800">
              {result.warnings.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}

      {error ? <p className="mt-4 rounded-xl border border-rose-300 bg-rose-50 px-4 py-3 text-sm text-rose-700">{error}</p> : null}
    </main>
  );
}
