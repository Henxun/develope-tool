"use client";

import { FormEvent, useEffect, useState } from "react";
import { useIsWindowsPlatform } from "@/lib/tools";
import { invokeTauri, toSinglePath } from "@/lib/tauri";

type ProgramMigrationResult = {
  appName: string;
  sourceDir: string;
  targetDir: string;
  moved: boolean;
  registryUpdates: number;
  shortcutUpdates: number;
  envVarUpdates: number;
  warnings: string[];
};

type ProgramMigrationRequest = {
  appName: string;
  sourceDir: string;
  targetDir: string;
  additionalRegistryKeys: string[];
  additionalShortcutDirs: string[];
  envVarNames: string[];
  includeMachineRegistry: boolean;
  dryRun: boolean;
};

type ProgramDetectResult = {
  normalizedSourceDir: string;
  registryKeys: string[];
  shortcutFiles: string[];
  envVarMatches: string[];
  warnings: string[];
};

function parseLines(raw: string): string[] {
  return raw
    .split(/\r?\n|,/)
    .map((value) => value.trim())
    .filter(Boolean);
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

function appendLeafDir(parentDir: string, sourceDir: string): string {
  const normalizedParent = parentDir.replace(/[\\/]+$/, "");
  const parts = sourceDir.split(/[\\/]/).filter(Boolean);
  const leaf = parts.at(-1);
  if (!leaf) return normalizedParent;
  return `${normalizedParent}\\${leaf}`;
}

function appendLeafDirWithSuffix(parentDir: string, sourceDir: string, suffix: string): string {
  const normalizedParent = parentDir.replace(/[\\/]+$/, "");
  const parts = sourceDir.split(/[\\/]/).filter(Boolean);
  const leaf = parts.at(-1);
  if (!leaf) return normalizedParent;
  return `${normalizedParent}\\${leaf}${suffix}`;
}

export default function WindowsMigrationToolPage() {
  const [appName, setAppName] = useState("");
  const [sourceDir, setSourceDir] = useState("");
  const [targetDir, setTargetDir] = useState("");
  const [registryKeys, setRegistryKeys] = useState("");
  const [shortcutDirs, setShortcutDirs] = useState("");
  const [envVarNames, setEnvVarNames] = useState("Path");
  const [includeMachineRegistry, setIncludeMachineRegistry] = useState(false);
  const [dryRun, setDryRun] = useState(true);

  const [loading, setLoading] = useState(false);
  const [elevating, setElevating] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [isElevated, setIsElevated] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<ProgramMigrationResult | null>(null);
  const [detectResult, setDetectResult] = useState<ProgramDetectResult | null>(null);
  const isWindows = useIsWindowsPlatform();

  const refreshElevationState = async () => {
    try {
      const elevated = await invokeTauri<boolean>("is_process_elevated");
      setIsElevated(elevated);
    } catch {
      setIsElevated(false);
    }
  };

  useEffect(() => {
    if (!isWindows) return;
    void refreshElevationState();
  }, [isWindows]);

  const handleElevate = async () => {
    setError("");
    setElevating(true);
    try {
      await invokeTauri("relaunch_as_admin");
    } catch (err) {
      setError(extractErrorMessage(err, "提权失败，请手动以管理员身份运行"));
      setElevating(false);
    }
  };

  const pickSourceDir = async () => {
    setError("");
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = toSinglePath(await open({ directory: true, multiple: false, title: "选择程序当前安装目录" }));
      if (selected) {
        const detectedAppName = appName.trim();
        setSourceDir(selected);
        setTargetDir((previous) => {
          if (previous.trim()) return previous;
          const parent = selected.replace(/[\\/]+[^\\/]+$/, "");
          return appendLeafDirWithSuffix(parent, selected, "-migrated");
        });
        void runDetection(selected, detectedAppName);
      }
    } catch (err) {
      setError(extractErrorMessage(err, "选择目录失败"));
    }
  };

  const pickTargetDir = async () => {
    setError("");
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = toSinglePath(await open({ directory: true, multiple: false, title: "选择迁移目标父目录" }));
      if (selected) {
        setTargetDir(appendLeafDir(selected, sourceDir.trim() || appName.trim() || "migrated-app"));
      }
    } catch (err) {
      setError(extractErrorMessage(err, "选择目录失败"));
    }
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setError("");
    setResult(null);

    if (!isWindows) {
      setError("该工具仅支持 Windows 平台");
      return;
    }
    if (!appName.trim() || !sourceDir.trim() || !targetDir.trim()) {
      setError("请至少填写应用名称、源目录、目标目录");
      return;
    }
    if (sourceDir.trim().toLowerCase() === targetDir.trim().toLowerCase()) {
      setError("目标目录不能与源目录相同，请改为新路径（例如追加 -migrated）");
      return;
    }

    const payload: ProgramMigrationRequest = {
      appName: appName.trim(),
      sourceDir: sourceDir.trim(),
      targetDir: targetDir.trim(),
      additionalRegistryKeys: parseLines(registryKeys),
      additionalShortcutDirs: parseLines(shortcutDirs),
      envVarNames: parseLines(envVarNames),
      includeMachineRegistry,
      dryRun,
    };

    setLoading(true);
    try {
      if (!dryRun) {
        const lockedFiles = await invokeTauri<string[]>("check_directory_locked", { dirPath: sourceDir.trim() });
        if (lockedFiles.length > 0) {
          const msg = lockedFiles.length <= 5
            ? lockedFiles.join("\n")
            : `${lockedFiles.slice(0, 5).join("\n")}\n...及其他 ${lockedFiles.length - 5} 个文件`;
          setError(`源目录被其它进程占用，请先关闭相关程序后重试：\n${msg}`);
          setLoading(false);
          return;
        }
      }

      const migrationResult = await invokeTauri<ProgramMigrationResult>("migrate_installed_program", { request: payload });
      setResult(migrationResult);
    } catch (err) {
      setError(extractErrorMessage(err, "程序迁移失败"));
    } finally {
      setLoading(false);
    }
  };

  const runDetection = async (source: string, app: string) => {
    if (!source.trim()) {
      setError("请先选择源目录再执行自动检测");
      return;
    }

    setError("");
    setDetecting(true);
    try {
      const payload = {
        appName: app.trim(),
        sourceDir: source.trim(),
        additionalShortcutDirs: parseLines(shortcutDirs),
        envVarNames: parseLines(envVarNames),
        includeMachineRegistry,
      };
      const data = await invokeTauri<ProgramDetectResult>("detect_program_references", { request: payload });
      setDetectResult(data);
      if (!registryKeys.trim() && data.registryKeys.length > 0) {
        setRegistryKeys(data.registryKeys.join("\n"));
      }
    } catch (err) {
      setError(extractErrorMessage(err, "自动检测失败"));
    } finally {
      setDetecting(false);
    }
  };

  if (!isWindows) {
    return (
      <main className="rounded-3xl border border-[var(--card-border)] bg-[var(--card)] p-8 shadow-[0_24px_65px_-35px_rgba(17,97,125,0.55)] backdrop-blur-xl">
        <h1 className="text-3xl font-semibold tracking-tight">Windows 程序迁移工具</h1>
        <p className="mt-4 text-sm text-slate-700">当前环境不是 Windows，该工具已隐藏实际功能，仅在 Windows 桌面端可用。</p>
      </main>
    );
  }

  return (
    <main className="rounded-3xl border border-[var(--card-border)] bg-[var(--card)] p-6 shadow-[0_24px_65px_-35px_rgba(17,97,125,0.55)] backdrop-blur-xl sm:p-8">
      <h1 className="text-3xl font-semibold tracking-tight">Windows 程序迁移工具</h1>
      <p className="mt-3 text-sm leading-7 text-slate-700">将已安装程序迁移到新目录，并尝试同步注册表、快捷方式、环境变量。建议先执行 dry-run。</p>

      <div className="mt-4 rounded-2xl border border-slate-200 bg-white/75 p-3 text-xs text-slate-700">
        <p>选择源目录后会自动检测相关信息。也可以手动点击“重新检测关联信息”。</p>
        <button
          type="button"
          onClick={() => void runDetection(sourceDir, appName)}
          disabled={detecting}
          className="mt-2 h-8 rounded-lg border border-slate-300 px-3 font-medium transition hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {detecting ? "检测中..." : "重新检测关联信息"}
        </button>
      </div>

      <form onSubmit={handleSubmit} className="mt-6 rounded-2xl border border-slate-200 bg-white/80 p-4">
        <div className="grid grid-cols-1 gap-3">
          <input
            value={appName}
            onChange={(event) => setAppName(event.target.value)}
            className="h-10 rounded-xl border border-slate-300 bg-white px-3 text-sm outline-none ring-[var(--accent)]/40 focus:ring"
            placeholder="应用名称（用于匹配卸载注册表）"
          />

          <input
            value={sourceDir}
            onChange={(event) => setSourceDir(event.target.value)}
            className="h-10 rounded-xl border border-slate-300 bg-white px-3 text-sm outline-none ring-[var(--accent)]/40 focus:ring"
            placeholder="源安装目录"
          />
          <button type="button" onClick={pickSourceDir} className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium hover:bg-slate-100">
            选择源目录
          </button>

          <input
            value={targetDir}
            onChange={(event) => setTargetDir(event.target.value)}
            className="h-10 rounded-xl border border-slate-300 bg-white px-3 text-sm outline-none ring-[var(--accent)]/40 focus:ring"
            placeholder="目标目录（通常为新父目录\\程序目录名）"
          />
          <button type="button" onClick={pickTargetDir} className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium hover:bg-slate-100">
            选择目标父目录
          </button>
          <button
            type="button"
            onClick={() => {
              if (!sourceDir.trim()) {
                setError("请先选择源目录");
                return;
              }
              const parent = sourceDir.replace(/[\\/]+[^\\/]+$/, "");
              setTargetDir(appendLeafDirWithSuffix(parent, sourceDir, "-migrated"));
              setError("");
            }}
            className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium hover:bg-slate-100"
          >
            生成同盘新目录
          </button>

          <textarea
            value={registryKeys}
            onChange={(event) => setRegistryKeys(event.target.value)}
            className="h-20 rounded-xl border border-slate-300 bg-white px-3 py-2 font-mono text-xs outline-none ring-[var(--accent)]/40 focus:ring"
            placeholder="附加注册表路径（可选，每行一个，例如 HKCU\\Software\\MyApp）"
          />
          <label className="flex items-center gap-2 text-sm text-slate-700">
            <input
              type="checkbox"
              checked={includeMachineRegistry}
              onChange={(event) => {
                setIncludeMachineRegistry(event.target.checked);
                if (event.target.checked) {
                  void refreshElevationState();
                }
              }}
            />
            同时尝试更新机器级注册表（HKLM，需要管理员权限）
          </label>
          {includeMachineRegistry ? (
            <div className="rounded-xl border border-slate-200 bg-slate-50 px-3 py-2 text-xs text-slate-700">
              <p>当前权限：{isElevated ? "管理员" : "普通用户"}</p>
              {!isElevated ? (
                <button
                  type="button"
                  onClick={handleElevate}
                  disabled={elevating}
                  className="mt-2 h-8 rounded-lg border border-slate-300 px-3 font-medium transition hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {elevating ? "请求提权中..." : "提权并重启"}
                </button>
              ) : null}
            </div>
          ) : null}
          <textarea
            value={shortcutDirs}
            onChange={(event) => setShortcutDirs(event.target.value)}
            className="h-20 rounded-xl border border-slate-300 bg-white px-3 py-2 font-mono text-xs outline-none ring-[var(--accent)]/40 focus:ring"
            placeholder="附加快捷方式扫描目录（可选，每行一个）"
          />
          <textarea
            value={envVarNames}
            onChange={(event) => setEnvVarNames(event.target.value)}
            className="h-16 rounded-xl border border-slate-300 bg-white px-3 py-2 font-mono text-xs outline-none ring-[var(--accent)]/40 focus:ring"
            placeholder="环境变量名称（每行一个，默认 Path）"
          />

          <label className="flex items-center gap-2 text-sm text-slate-700">
            <input type="checkbox" checked={dryRun} onChange={(event) => setDryRun(event.target.checked)} />
            仅模拟（dry-run，不真正移动文件）
          </label>

          <button
            type="submit"
            disabled={loading}
            className="h-10 rounded-xl bg-[var(--accent)] px-4 text-sm font-medium text-white transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {loading ? "执行中..." : dryRun ? "开始模拟迁移" : "开始真实迁移"}
          </button>
        </div>
      </form>

      {detectResult ? (
        <div className="mt-4 rounded-2xl border border-sky-200 bg-sky-50 p-4 text-sm text-sky-900">
          <p className="font-medium">自动检测结果</p>
          <p className="mt-1 text-xs">源目录：{detectResult.normalizedSourceDir}</p>
          <p className="mt-2 text-xs">注册表关联：{detectResult.registryKeys.length} 项</p>
          <p className="text-xs">快捷方式关联：{detectResult.shortcutFiles.length} 项</p>
          <p className="text-xs">环境变量关联：{detectResult.envVarMatches.length} 项</p>

          {detectResult.registryKeys.length ? (
            <div className="mt-3">
              <p className="text-xs font-semibold">注册表键</p>
              <ul className="mt-1 max-h-28 overflow-auto rounded border border-sky-100 bg-white p-2 font-mono text-[11px]">
                {detectResult.registryKeys.map((value) => (
                  <li key={value}>{value}</li>
                ))}
              </ul>
            </div>
          ) : null}

          {detectResult.shortcutFiles.length ? (
            <div className="mt-3">
              <p className="text-xs font-semibold">快捷方式文件</p>
              <ul className="mt-1 max-h-28 overflow-auto rounded border border-sky-100 bg-white p-2 font-mono text-[11px]">
                {detectResult.shortcutFiles.map((value) => (
                  <li key={value}>{value}</li>
                ))}
              </ul>
            </div>
          ) : null}

          {detectResult.envVarMatches.length ? (
            <div className="mt-3">
              <p className="text-xs font-semibold">环境变量</p>
              <ul className="mt-1 rounded border border-sky-100 bg-white p-2 font-mono text-[11px]">
                {detectResult.envVarMatches.map((value) => (
                  <li key={value}>{value}</li>
                ))}
              </ul>
            </div>
          ) : null}

          {detectResult.warnings.length ? (
            <ul className="mt-3 list-disc space-y-1 pl-5 text-xs text-amber-800">
              {detectResult.warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}

      {result ? (
        <div className="mt-4 rounded-2xl border border-emerald-200 bg-emerald-50 p-4 text-sm text-emerald-800">
          <p>
            完成：{result.appName}，目录 {result.sourceDir} -&gt; {result.targetDir}
          </p>
          <p className="mt-1">移动目录：{result.moved ? "是" : "否（dry-run）"}</p>
          <p className="mt-1">
            注册表更新 {result.registryUpdates} 项，快捷方式更新 {result.shortcutUpdates} 项，环境变量更新 {result.envVarUpdates} 项
          </p>
          {result.warnings.length ? (
            <ul className="mt-2 list-disc space-y-1 pl-5 text-xs text-amber-800">
              {result.warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : null}

      {error ? <p className="mt-4 rounded-xl border border-rose-300 bg-rose-50 px-4 py-3 text-sm text-rose-700">{error}</p> : null}
    </main>
  );
}
