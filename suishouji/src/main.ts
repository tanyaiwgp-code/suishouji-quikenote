// ============================================================
// 随手记 · 应用入口
// M0: 骨架搭建 — 主题切换 + 基础生命周期
// M2: 主窗口三栏 UI — 加载列表、搜索、导航、新建、文件监听刷新
// ============================================================

import "./styles.css";
import { listNotes, onNotesChanged, writeNote } from "./lib/api";
import { createEditor, type EditorInstance } from "./lib/editor";
import { index, mobileView, navView, notes, query, selectedPath, type NavView } from "./lib/store";
import { initTheme } from "./lib/theme";
import { applyViewMode, renderAll, renderShell, setEditor, updateListTitle } from "./ui";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { NoteMeta } from "./types";

/** 与 Rust `list_notes` 一致的排序：置顶 → mtime 倒序 → 路径字典序（B7 本地重排用）。 */
function byMtime(a: NoteMeta, b: NoteMeta): number {
  return Number(b.pinned) - Number(a.pinned) || b.mtime - a.mtime || a.path.localeCompare(b.path);
}

// --- 数据加载 ---
async function loadNotes(): Promise<void> {
  try {
    const data = await listNotes();
    notes.set(data);
    index.build(data);
  } catch (e) {
    console.error("加载笔记失败:", e);
    notes.set([]);
  }
  updateListTitle();
  renderAll();
  applyViewMode();
}

// --- 新建笔记（收件箱，时间戳命名） ---
// B5：秒级时间戳同秒连续点击会碰撞同名文件 → 会话内序号 + 与现有列表查重兜底
let lastCreateBase = "";
let createSeq = 0;

async function createNote(): Promise<void> {
  const ts = new Date();
  const pad = (x: number) => String(x).padStart(2, "0");
  const base =
    `收件箱/${ts.getFullYear()}-${pad(ts.getMonth() + 1)}-${pad(ts.getDate())}` +
    `_${pad(ts.getHours())}${pad(ts.getMinutes())}${pad(ts.getSeconds())}`;
  if (base === lastCreateBase) {
    createSeq += 1;
  } else {
    lastCreateBase = base;
    createSeq = 0;
  }
  let rel = createSeq === 0 ? `${base}.md` : `${base}_${createSeq}.md`;
  // 兜底：与磁盘上已存在的笔记路径查重（跨会话同名文件）
  const used = new Set(notes.get().map((n) => n.path));
  while (used.has(rel)) {
    createSeq += 1;
    rel = `${base}_${createSeq}.md`;
  }
  try {
    await writeNote(rel, "");
    selectedPath.set(rel);
    await loadNotes();
    if (window.matchMedia("(max-width: 719px)").matches) {
      mobileView.set("editor");
      applyViewMode();
    }
  } catch (e) {
    console.error("新建笔记失败:", e);
  }
}

// --- 事件绑定 ---
function wireEvents(editor: EditorInstance): void {
  // 主题切换
  document.getElementById("theme-toggle")?.addEventListener("click", () => {
    const cur = document.documentElement.getAttribute("data-theme");
    const next = cur === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", next);
    localStorage.setItem("theme", next);
  });

  // 搜索框（120ms 防抖）
  const search = document.getElementById("search-input") as HTMLInputElement | null;
  let timer = 0;
  search?.addEventListener("input", () => {
    clearTimeout(timer);
    timer = window.setTimeout(() => {
      query.set(search.value);
      renderAll();
    }, 120);
  });

  // 导航（全部 / 收藏）
  document.querySelectorAll(".nav-item[data-view]").forEach((btn) => {
    btn.addEventListener("click", () => {
      navView.set((btn as HTMLElement).dataset.view as NavView);
      document
        .querySelectorAll(".nav-item[data-view]")
        .forEach((b) => b.classList.toggle("active", b === btn));
      updateListTitle();
      renderAll();
    });
  });

  // 新建
  document.getElementById("new-note")?.addEventListener("click", () => void createNote());

  // 笔记列表点击（事件委托）
  document.getElementById("note-list")?.addEventListener("click", (e) => {
    const card = (e.target as HTMLElement).closest<HTMLElement>(".note-card");
    if (!card) return;
    selectedPath.set(card.dataset.path ?? null);
    if (window.matchMedia("(max-width: 719px)").matches) {
      mobileView.set("editor");
    }
    applyViewMode();
    renderAll();
  });

  // 文件系统监听：外部改动 → 自动刷新列表（M2-4）+ 当前打开笔记重载（M3）
  onNotesChanged(async () => {
    await loadNotes();
    await editor.onExternalChange();
  }).catch(() => {
    /* 非 Tauri 环境（纯 vite）下忽略 */
  });

  // 单栏模式下「返回列表」（编辑器壳内 #ed-back 派发）
  window.addEventListener("editor:back", () => {
    mobileView.set("list");
    applyViewMode();
  });

  // B7：编辑器自动保存成功 → 本地更新 mtime 并重排，无需 watcher 全量重扫
  window.addEventListener("note:saved", (e) => {
    const rel = (e as CustomEvent<{ rel: string }>).detail?.rel;
    if (!rel) return;
    const arr = notes.get();
    const i = arr.findIndex((x) => x.path === rel);
    if (i < 0) return;
    const copy = arr.slice();
    copy[i] = { ...copy[i], mtime: Date.now() };
    copy.sort(byMtime);
    notes.set(copy);
    renderAll();
  });

  // 窗口尺寸变化时校正单栏视图模式
  window.addEventListener("resize", applyViewMode);
}

// --- 关窗落盘（B2，自 M4 前置起由窗口入口接线） ---
// 拦截关窗请求 → 等待保存完成再隐藏到托盘（进程存活），避免异步 IPC 未落地就关窗丢字。
// 非 Tauri（纯 vite）退化为 beforeunload 尽力而为。真正退出走托盘「退出」。
function wireWindowClose(editor: EditorInstance): void {
  if (isTauri()) {
    getCurrentWindow()
      .onCloseRequested(async (event) => {
        event.preventDefault();
        await editor.flushSave(); // flushSave() 内部已 clearTimeout(saveTimer)
        await getCurrentWindow().hide().catch(() => {});
      })
      .catch(() => {
        /* 无窗口环境忽略 */
      });
  }
  window.addEventListener("beforeunload", () => {
    void editor.flushSave();
  });
}

// --- 启动 ---
window.addEventListener("DOMContentLoaded", () => {
  initTheme();
  renderShell();
  const editor = createEditor(document.getElementById("editor") as HTMLElement);
  setEditor(editor);
  wireEvents(editor);
  wireWindowClose(editor);
  void loadNotes();
});
