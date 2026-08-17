// 轻量状态（nanostores）。M2 的 UI 直接订阅/读取这些 atom。
import { atom } from "nanostores";
import type { NoteMeta, TrashEntry } from "../types";
import { NoteIndex } from "./search";

export const notes = atom<NoteMeta[]>([]);
/** P0-数据安全：回收站条目列表（navView === "trash" 时展示）。 */
export const trash = atom<TrashEntry[]>([]);
export const selectedPath = atom<string | null>(null);
export const query = atom<string>("");
export type NavView = "all" | "pinned" | "trash";
export const navView = atom<NavView>("all");
export type MobileView = "list" | "editor";
export const mobileView = atom<MobileView>("list");

/** 搜索倒排索引（M2-3），随 notes 重建。 */
export const index = new NoteIndex();

/**
 * M7：删除笔记后的本地状态更新——列表移除 + 重建搜索索引 + 若删的是当前选中则清空选中。
 * 纯逻辑，供 main.ts 删除流程调用并独立单测（node 环境无 DOM）。
 */
export function applyDelete(rel: string): void {
  notes.set(notes.get().filter((n) => n.path !== rel));
  index.build(notes.get());
  if (selectedPath.get() === rel) selectedPath.set(null);
}
