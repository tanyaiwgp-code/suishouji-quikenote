// 类型化 IPC 封装（薄封装，命令名与 Rust commands.rs / build.rs 白名单一致）
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { NoteMeta } from "../types";

export const listNotes = (): Promise<NoteMeta[]> => invoke<NoteMeta[]>("list_notes");
export const readNote = (rel: string): Promise<string> => invoke<string>("read_note", { rel });
export const writeNote = (rel: string, content: string): Promise<void> =>
  invoke<void>("write_note", { rel, content });
export const deleteNote = (rel: string): Promise<void> => invoke<void>("delete_note", { rel });
export const acquireNoteLock = (rel: string): Promise<void> =>
  invoke<void>("acquire_note_lock", { rel });
export const releaseNoteLock = (rel: string): Promise<void> =>
  invoke<void>("release_note_lock", { rel });

// --- M3：图片导入 & 绝对路径 ---

export interface AssetImport {
  rel: string;
  count: number;
}

/** 拖入图片：Rust 复制到 `笔记名/assets/`，返回相对引用。 */
export const assetsImport = (noteRel: string, sourcePath: string): Promise<AssetImport> =>
  invoke<AssetImport>("assets_import", { noteRel, sourcePath });

/** 工具栏选图：base64 字节写入 `笔记名/assets/`，返回相对引用。 */
export const assetsImportBase64 = (
  noteRel: string,
  filename: string,
  data: string,
): Promise<AssetImport> => invoke<AssetImport>("assets_import_base64", { noteRel, filename, data });

/** 返回笔记绝对路径（供 convertFileSrc 渲染 assets/ 图片）。 */
export const noteAbsPath = (rel: string): Promise<string> => invoke<string>("note_abs_path", { rel });

/** 导出 convertFileSrc，供预览图片路径重写。 */
export { convertFileSrc };

/** 订阅笔记目录外部改动事件（M2-4），返回取消订阅函数。 */
export function onNotesChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("notes://changed", () => cb());
}

// --- M4：窗口控制（Rust 端执行，避免给浮窗授予逐窗口 JS 权限） ---

/** 显示并聚焦主窗口（同时隐藏快速记录浮窗）。 */
export const openMainWindow = (): Promise<void> => invoke<void>("open_main_window");
/** 隐藏快速记录浮窗。 */
export const hideQuicknote = (): Promise<void> => invoke<void>("hide_quicknote");
