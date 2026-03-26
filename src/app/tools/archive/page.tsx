"use client";

import { FormEvent, useState } from "react";
import {
  archiveFilterExtension,
  invokeTauri,
  parseFilters,
  suggestedArchiveExtension,
  toSinglePath,
} from "@/lib/tauri";

type ArchiveResult = {
  outputPath: string;
  processedCount: number;
  skippedCount: number;
  format: string;
};

const archiveFormats = ["zip", "tar", "tar.gz", "tar.bz2", "tar.xz"];

export default function ArchiveToolPage() {
  const [compressSource, setCompressSource] = useState("");
  const [compressOutput, setCompressOutput] = useState("");
  const [compressFormat, setCompressFormat] = useState("zip");
  const [compressExcludes, setCompressExcludes] = useState("node_modules/**\n.git/**\n*.log");
  const [compressLoading, setCompressLoading] = useState(false);

  const [extractArchivePath, setExtractArchivePath] = useState("");
  const [extractOutputDir, setExtractOutputDir] = useState("");
  const [extractExcludes, setExtractExcludes] = useState("*.map\n*.tmp");
  const [extractLoading, setExtractLoading] = useState(false);

  const [error, setError] = useState("");
  const [compressResult, setCompressResult] = useState<ArchiveResult | null>(null);
  const [extractResult, setExtractResult] = useState<ArchiveResult | null>(null);

  const handleCompress = async (event: FormEvent) => {
    event.preventDefault();
    setError("");
    setCompressResult(null);

    if (!compressSource.trim() || !compressOutput.trim()) {
      setError("压缩时请填写源路径和输出路径");
      return;
    }

    setCompressLoading(true);
    try {
      const result = await invokeTauri<ArchiveResult>("compress_archive", {
        sourcePath: compressSource.trim(),
        outputPath: compressOutput.trim(),
        format: compressFormat,
        excludes: parseFilters(compressExcludes),
      });
      setCompressResult(result);
    } catch (err) {
      setError(err instanceof Error ? err.message : "压缩失败");
    } finally {
      setCompressLoading(false);
    }
  };

  const handleExtract = async (event: FormEvent) => {
    event.preventDefault();
    setError("");
    setExtractResult(null);

    if (!extractArchivePath.trim() || !extractOutputDir.trim()) {
      setError("解压时请填写压缩包路径和解压目录");
      return;
    }

    setExtractLoading(true);
    try {
      const result = await invokeTauri<ArchiveResult>("extract_archive", {
        archivePath: extractArchivePath.trim(),
        outputDir: extractOutputDir.trim(),
        excludes: parseFilters(extractExcludes),
      });
      setExtractResult(result);
    } catch (err) {
      setError(err instanceof Error ? err.message : "解压失败");
    } finally {
      setExtractLoading(false);
    }
  };

  const pickCompressSourceFile = async () => {
    setError("");
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = toSinglePath(await open({ multiple: false, directory: false, title: "选择要压缩的文件" }));
      if (selected) setCompressSource(selected);
    } catch (err) {
      setError(err instanceof Error ? err.message : "选择文件失败");
    }
  };

  const pickCompressSourceDirectory = async () => {
    setError("");
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = toSinglePath(await open({ multiple: false, directory: true, title: "选择要压缩的文件夹" }));
      if (selected) setCompressSource(selected);
    } catch (err) {
      setError(err instanceof Error ? err.message : "选择文件夹失败");
    }
  };

  const pickCompressOutput = async () => {
    setError("");
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const selected = await save({
        title: "选择压缩包保存位置",
        defaultPath: `archive.${suggestedArchiveExtension(compressFormat)}`,
        filters: [
          {
            name: `${compressFormat} archive`,
            extensions: [archiveFilterExtension(compressFormat)],
          },
        ],
      });
      if (selected) setCompressOutput(selected);
    } catch (err) {
      setError(err instanceof Error ? err.message : "选择输出路径失败");
    }
  };

  const pickExtractArchive = async () => {
    setError("");
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = toSinglePath(
        await open({
          multiple: false,
          directory: false,
          title: "选择压缩包",
          filters: [{ name: "Archives", extensions: ["zip", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz"] }],
        }),
      );
      if (selected) setExtractArchivePath(selected);
    } catch (err) {
      setError(err instanceof Error ? err.message : "选择压缩包失败");
    }
  };

  const pickExtractOutputDirectory = async () => {
    setError("");
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = toSinglePath(await open({ multiple: false, directory: true, title: "选择解压输出目录" }));
      if (selected) setExtractOutputDir(selected);
    } catch (err) {
      setError(err instanceof Error ? err.message : "选择输出目录失败");
    }
  };

  return (
    <main className="rounded-3xl border border-[var(--card-border)] bg-[var(--card)] p-6 shadow-[0_24px_65px_-35px_rgba(17,97,125,0.55)] backdrop-blur-xl sm:p-8">
      <h1 className="text-3xl font-semibold tracking-tight">压缩 / 解压工具</h1>
      <p className="mt-3 text-sm leading-7 text-slate-700">支持过滤规则（Glob）排除文件或文件夹，如 `node_modules/**`、`*.log`。</p>

      <div className="mt-7 grid grid-cols-1 gap-4 lg:grid-cols-2">
        <form onSubmit={handleCompress} className="rounded-2xl border border-slate-200 bg-white/75 p-4">
          <h2 className="text-lg font-semibold">压缩</h2>
          <div className="mt-3 space-y-3 text-sm">
            <input
              value={compressSource}
              onChange={(event) => setCompressSource(event.target.value)}
              className="h-10 w-full rounded-xl border border-slate-300 bg-white px-3 outline-none ring-[var(--accent)]/40 transition focus:ring"
              placeholder="源路径（文件或文件夹）"
            />
            <div className="flex gap-2">
              <button type="button" onClick={pickCompressSourceFile} className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium transition hover:bg-slate-100">
                选择文件
              </button>
              <button
                type="button"
                onClick={pickCompressSourceDirectory}
                className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium transition hover:bg-slate-100"
              >
                选择文件夹
              </button>
            </div>
            <input
              value={compressOutput}
              onChange={(event) => setCompressOutput(event.target.value)}
              className="h-10 w-full rounded-xl border border-slate-300 bg-white px-3 outline-none ring-[var(--accent)]/40 transition focus:ring"
              placeholder="输出压缩包路径"
            />
            <button
              type="button"
              onClick={pickCompressOutput}
              className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium transition hover:bg-slate-100"
            >
              选择保存位置
            </button>
            <select
              value={compressFormat}
              onChange={(event) => setCompressFormat(event.target.value)}
              className="h-10 w-full rounded-xl border border-slate-300 bg-white px-3 outline-none ring-[var(--accent)]/40 transition focus:ring"
            >
              {archiveFormats.map((format) => (
                <option key={format} value={format}>
                  {format}
                </option>
              ))}
            </select>
            <textarea
              value={compressExcludes}
              onChange={(event) => setCompressExcludes(event.target.value)}
              className="h-24 w-full rounded-xl border border-slate-300 bg-white px-3 py-2 font-mono text-xs outline-none ring-[var(--accent)]/40 transition focus:ring"
              placeholder="每行一个过滤条件（支持逗号分隔）"
            />
            <button
              type="submit"
              disabled={compressLoading}
              className="h-10 rounded-xl bg-[var(--accent)] px-4 font-medium text-white transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {compressLoading ? "压缩中..." : "开始压缩"}
            </button>
          </div>
          {compressResult ? (
            <p className="mt-3 rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 text-xs text-emerald-700">
              已生成 {compressResult.format} 文件，处理 {compressResult.processedCount} 项，排除 {compressResult.skippedCount} 项。
            </p>
          ) : null}
        </form>

        <form onSubmit={handleExtract} className="rounded-2xl border border-slate-200 bg-white/75 p-4">
          <h2 className="text-lg font-semibold">解压</h2>
          <div className="mt-3 space-y-3 text-sm">
            <input
              value={extractArchivePath}
              onChange={(event) => setExtractArchivePath(event.target.value)}
              className="h-10 w-full rounded-xl border border-slate-300 bg-white px-3 outline-none ring-[var(--accent)]/40 transition focus:ring"
              placeholder="压缩包路径"
            />
            <button
              type="button"
              onClick={pickExtractArchive}
              className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium transition hover:bg-slate-100"
            >
              选择压缩包
            </button>
            <input
              value={extractOutputDir}
              onChange={(event) => setExtractOutputDir(event.target.value)}
              className="h-10 w-full rounded-xl border border-slate-300 bg-white px-3 outline-none ring-[var(--accent)]/40 transition focus:ring"
              placeholder="解压输出目录"
            />
            <button
              type="button"
              onClick={pickExtractOutputDirectory}
              className="h-9 rounded-lg border border-slate-300 px-3 text-xs font-medium transition hover:bg-slate-100"
            >
              选择解压目录
            </button>
            <textarea
              value={extractExcludes}
              onChange={(event) => setExtractExcludes(event.target.value)}
              className="h-24 w-full rounded-xl border border-slate-300 bg-white px-3 py-2 font-mono text-xs outline-none ring-[var(--accent)]/40 transition focus:ring"
              placeholder="解压排除条件（每行一个）"
            />
            <button
              type="submit"
              disabled={extractLoading}
              className="h-10 rounded-xl border border-slate-300 px-4 font-medium transition hover:bg-slate-100 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {extractLoading ? "解压中..." : "开始解压"}
            </button>
          </div>
          {extractResult ? (
            <p className="mt-3 rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 text-xs text-emerald-700">
              已解压 {extractResult.format} 文件，处理 {extractResult.processedCount} 项，排除 {extractResult.skippedCount} 项。
            </p>
          ) : null}
        </form>
      </div>

      {error ? <p className="mt-5 rounded-xl border border-rose-300 bg-rose-50 px-4 py-3 text-sm text-rose-700">{error}</p> : null}
    </main>
  );
}
