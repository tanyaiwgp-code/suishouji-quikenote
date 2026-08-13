// 类型化 IPC 封装（薄封装，命令名与 Rust commands.rs / build.rs 白名单一致）
import { invoke } from "@tauri-apps/api/core";
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

/** 订阅笔记目录外部改动事件（M2-4），返回取消订阅函数。 */
export function onNotesChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("notes://changed", () => cb());
}
