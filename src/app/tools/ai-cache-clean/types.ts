import type { AiCacheCategoryInfo, AiToolScan } from "@/lib/tauri";

/** 同一工具多个根目录（VS Code 系）合并后的视图模型。 */
export type ToolView = AiToolScan & {
  categories: AiCacheCategoryInfo[];
  /** 实际存在的根目录，供「打开目录」使用。 */
  rootPaths: string[];
};
