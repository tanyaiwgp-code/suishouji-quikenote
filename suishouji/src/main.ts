// ============================================================
// 随手记 · 应用入口
// M0: 骨架搭建 — 主题切换 + 基础生命周期
// M2: 主窗口三栏 UI — 加载列表、搜索、导航、新建、文件监听刷新
// ============================================================

import "./styles.css";
import { listNotes, onNotesChanged, writeNote } from "./lib/api";
import { onExternalChange } from "./lib/editor";
import { index, mobileView, navView, notes, query, selectedPath, type NavView } from "./lib/store";
import { applyViewMode, renderAll, renderShell, updateListTitle } from "./ui";

// --- 主题管理 ---
function initTheme(): void {
  const stored = localStorage.getItem("theme");
  if (stored === "dark" || stored === "light") {
    document.documentElement.setAttribute("data-theme", stored);
  }
  // 否则跟随系统（CSS media query 自动处理）
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
async function createNote(): Promise<void> {
  const ts = new Date();
  const pad = (x: number) => String(x).padStart(2, "0");
  const rel =
    `收件箱/${ts.getFullYear()}-${pad(ts.getMonth() + 1)}-${pad(ts.getDate())}` +
    `_${pad(ts.getHours())}${pad(ts.getMinutes())}${pad(ts.getSeconds())}.md`;
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
function wireEvents(): void {
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
    await onExternalChange();
  }).catch(() => {
    /* 非 Tauri 环境（纯 vite）下忽略 */
  });

  // 单栏模式下「返回列表」（编辑器壳内 #ed-back 派发）
  window.addEventListener("editor:back", () => {
    mobileView.set("list");
    applyViewMode();
  });

  // 窗口尺寸变化时校正单栏视图模式
  window.addEventListener("resize", applyViewMode);
}

// --- 启动 ---
window.addEventListener("DOMContentLoaded", () => {
  initTheme();
  renderShell();
  wireEvents();
  void loadNotes();
});
