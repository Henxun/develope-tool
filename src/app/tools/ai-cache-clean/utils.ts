import type { AiCacheCategory } from "@/lib/tauri";

export const CATEGORY_LABELS: Record<AiCacheCategory, string> = {
  cache: "缓存",
  logs: "日志",
  sessions: "会话记录",
  model_cache: "模型文件",
};

/** 把后端 `CODE: 中文文案` 错误转成可直接展示的文案。 */
export function humanizeError(message: string | null): string {
  if (!message) return "操作失败";
  const index = message.indexOf(": ");
  return index >= 0 ? message.slice(index + 2) : message;
}
