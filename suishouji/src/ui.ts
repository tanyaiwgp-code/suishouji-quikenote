// 主窗口渲染：静态骨架 + 笔记列表 + 编辑器占位 + 空状态。
// 轻渲染：状态变更时整体重绘（200 条规模内代价可忽略）。

import { index, mobileView, navView, notes, query, selectedPath } from "./lib/store";
import type { EditorInstance } from "./lib/editor";
import type { NoteMeta } from "./types";

const $ = <T extends HTMLElement>(sel: string): T => document.querySelector(sel) as T;

let editor: EditorInstance | null = null;

/** main.ts 在 renderShell() 之后注入编辑器实例。 */
export function setEditor(ed: EditorInstance): void {
  editor = ed;
}

/** 首次构建静态骨架（顶栏 + 三栏容器）。事件绑定在 main.ts。 */
export function renderShell(): void {
  const app = $("#app");
  app.innerHTML = `
    <div class="app">
      <header class="topbar">
        <div class="brand">随手记</div>
        <div class="search">
          <input id="search-input" type="search" placeholder="搜索笔记…" aria-label="搜索笔记" autocomplete="off" />
        </div>
        <div class="topbar-actions">
          <button class="btn-icon" id="theme-toggle" title="切换主题" aria-label="切换主题">◐</button>
          <button class="btn-icon" id="settings-btn" title="设置（M6）" aria-label="设置" disabled>⚙</button>
        </div>
      </header>
      <div class="workspace">
        <nav class="col-nav" aria-label="导航">
          <button class="nav-item active" data-view="all" title="全部" aria-label="全部笔记">≡</button>
          <button class="nav-item" data-view="pinned" title="收藏" aria-label="收藏">☆</button>
          <button class="nav-item" title="标签（后续版本）" aria-label="标签" disabled>#</button>
          <span class="nav-grow"></span>
          <button class="nav-item accent" id="new-note" title="新建笔记" aria-label="新建笔记">＋</button>
        </nav>
        <aside class="col-list">
          <div class="list-head">
            <span id="list-title" class="list-title">全部笔记</span>
            <span id="list-count" class="count"></span>
          </div>
          <ul id="note-list" class="note-list"></ul>
        </aside>
        <main class="col-editor" id="editor" aria-label="编辑器"></main>
      </div>
    </div>
  `;
}

/** 重绘列表与编辑器（状态变更后调用）。 */
export function renderAll(): void {
  renderList();
  renderEditor();
}

function filteredNotes(): NoteMeta[] {
  const all = notes.get();
  const view = navView.get();
  const q = query.get().trim();

  let list = all;
  if (view === "pinned") list = list.filter((n) => n.pinned);

  if (q) {
    const rank = new Map<string, number>();
    index.search(q).forEach((p, i) => rank.set(p, -i)); // 保持命中按权重倒序
    list = list
      .filter((n) => rank.has(n.path))
      .sort((a, b) => (rank.get(a.path)! - rank.get(b.path)!));
  }
  return list;
}

function renderList(): void {
  const listEl = $("#note-list");
  const countEl = $("#list-count");
  const items = filteredNotes();
  countEl.textContent = String(items.length);

  if (items.length === 0) {
    listEl.innerHTML = emptyHtml(notes.get().length === 0);
    return;
  }
  const sel = selectedPath.get();
  listEl.innerHTML = items.map((n) => cardHtml(n, n.path === sel)).join("");
}

function cardHtml(n: NoteMeta, selected: boolean): string {
  const title = n.title.trim() ? n.title : "（无标题）";
  const preview = n.preview.trim() ? n.preview : "空笔记";
  const tags = n.tags.length ? `<span class="card-tags">#${escapeHtml(n.tags.join(" #"))}</span>` : "";
  return `
    <li class="note-card${selected ? " selected" : ""}" data-path="${escapeAttr(n.path)}">
      <div class="card-top">
        <span class="card-title">${escapeHtml(title)}</span>
        <span class="badge badge-${n.format}">${n.format.toUpperCase()}</span>
      </div>
      <div class="card-preview">${escapeHtml(preview)}</div>
      <div class="card-meta">
        <span>${formatTime(n.mtime)}</span>
        ${n.imageCount ? `<span>· ${n.imageCount} 图</span>` : ""}
        ${tags}
        ${n.pinned ? `<span class="pinned-dot" title="已置顶">☆</span>` : ""}
      </div>
    </li>`;
}

function emptyHtml(noNotes: boolean): string {
  if (noNotes) {
    return `
      <div class="empty-state">
        <div class="empty-illus">📝</div>
        <div class="empty-title">还没有笔记</div>
        <div class="empty-hint">点击 ＋ 新建，或按 <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>N</kbd> 快速记录</div>
      </div>`;
  }
  return `<div class="empty-state"><div class="empty-title">没有匹配的笔记</div><div class="empty-hint">换个关键词试试</div></div>`;
}

function renderEditor(): void {
  const sel = selectedPath.get();
  const n = sel ? notes.get().find((x) => x.path === sel) : undefined;
  editor?.render(n ?? null);
}

/** 单栏（<720px）列表 ⇄ 编辑器切换。 */
export function applyViewMode(): void {
  if (!window.matchMedia("(max-width: 719px)").matches) return;
  document.body.classList.toggle("view-editor", mobileView.get() === "editor");
}

export function updateListTitle(): void {
  $("#list-title").textContent = navView.get() === "pinned" ? "收藏" : "全部笔记";
}

// ---------- 工具 ----------

function formatTime(ms: number): string {
  if (!ms) return "";
  const d = new Date(ms);
  const now = new Date();
  const pad = (x: number) => String(x).padStart(2, "0");
  if (d.toDateString() === now.toDateString()) {
    return `今天 ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }
  if (d.getFullYear() === now.getFullYear()) {
    return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!
  );
}

function escapeAttr(s: string): string {
  return escapeHtml(s).replace(/`/g, "&#96;");
}
