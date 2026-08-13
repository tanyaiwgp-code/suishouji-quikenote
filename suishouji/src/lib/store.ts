// 轻量状态（nanostores）。M2 的 UI 直接订阅/读取这些 atom。
import { atom } from "nanostores";
import type { NoteMeta } from "../types";
import { NoteIndex } from "./search";

export const notes = atom<NoteMeta[]>([]);
export const selectedPath = atom<string | null>(null);
export const query = atom<string>("");
export type NavView = "all" | "pinned";
export const navView = atom<NavView>("all");
export type MobileView = "list" | "editor";
export const mobileView = atom<MobileView>("list");

/** 搜索倒排索引（M2-3），随 notes 重建。 */
export const index = new NoteIndex();
