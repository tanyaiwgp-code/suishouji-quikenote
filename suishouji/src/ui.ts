// 主窗口渲染：静态骨架 + 笔记列表 + 编辑器占位 + 空状态。
// 轻渲染：状态变更时整体重绘（200 条规模内代价可忽略）。

import { index, mobileView, navView, notes, query, selectedPath, trash } from "./lib/store";
import type { EditorInstance } from "./lib/editor";
import type { NoteMeta, TrashEntry } from "./types";

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
          <button class="btn-icon" id="settings-btn" title="设置" aria-label="设置" aria-haspopup="dialog">⚙</button>
        </div>
      </header>
      <div class="workspace">
        <nav class="col-nav" aria-label="导航">
          <button class="nav-item active" data-view="all" title="全部" aria-label="全部笔记">≡</button>
          <button class="nav-item" data-view="pinned" title="收藏" aria-label="收藏">☆</button>
          <button class="nav-item" data-view="trash" title="回收站" aria-label="回收站">🗑</button>
          <span class="nav-grow"></span>
          <span class="new-menu-wrap">
            <button class="nav-item accent" id="new-note" title="新建笔记" aria-label="新建笔记" aria-haspopup="menu" aria-expanded="false" aria-controls="new-menu">＋</button>
            <div class="new-menu" id="new-menu" role="menu" hidden>
              <button type="button" role="menuitem" data-ext="md">新建 Markdown</button>
              <button type="button" role="menuitem" data-ext="txt">新建纯文本</button>
            </div>
          </span>
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
    <div class="modal-backdrop" id="settings-backdrop" hidden>
      <div class="modal" role="dialog" aria-modal="true" aria-label="设置">
        <header class="modal-header">
          <h2 class="modal-title">设置</h2>
          <button class="btn-icon" id="settings-close" title="关闭" aria-label="关闭设置">×</button>
        </header>
        <div class="modal-body">
          <div class="setting-row">
            <span class="setting-label">数据目录</span>
            <div class="setting-control">
              <span id="set-root" class="setting-value" title="当前数据根目录">…</span>
              <button id="set-root-pick" class="btn-secondary">选择…</button>
            </div>
          </div>
          <div class="setting-row">
            <label class="setting-label" for="set-theme">主题</label>
            <select id="set-theme" class="setting-select">
              <option value="system">跟随系统</option>
              <option value="light">浅色</option>
              <option value="dark">深色</option>
            </select>
          </div>
          <div class="setting-row">
            <label class="setting-label" for="set-autostart">开机自启</label>
            <input type="checkbox" id="set-autostart" class="setting-checkbox" />
          </div>
          <div class="setting-row">
            <label class="setting-label" for="set-font">字号</label>
            <select id="set-font" class="setting-select">
              <option value="small">小</option>
              <option value="standard">标准</option>
              <option value="large">大</option>
            </select>
          </div>
          <!-- P0-数据安全：OneDrive 重定向提示（由 main.ts 检测后显示） -->
          <div class="setting-row warn-row" id="onedrive-warn" hidden>
            <span class="setting-label">⚠ 提示</span>
            <div class="setting-control setting-value">
              数据目录在 OneDrive 同步范围内。请留意云端同步可能与本地写入冲突，建议改为纯本地目录（见「数据目录」）。
            </div>
          </div>
          <!-- P0-数据安全：备份 / 恢复 -->
          <div class="setting-row">
            <span class="setting-label">数据安全</span>
            <div class="setting-control setting-actions">
              <button id="set-backup" class="btn-secondary">备份全部…</button>
              <button id="set-restore" class="btn-secondary">从备份恢复…</button>
            </div>
          </div>
          <!-- P0-商用化：关于区（版本 / 日志目录 / 检查更新） -->
          <div class="setting-row">
            <span class="setting-label">关于</span>
            <div class="setting-control setting-actions">
              <span id="set-version" class="setting-value">随手记</span>
              <button id="set-log-dir" class="btn-secondary">打开日志目录</button>
              <button id="set-check-update" class="btn-secondary">检查更新</button>
            </div>
          </div>
        </div>
      </div>
    </div>
    <!-- M7：删除确认对话框（复用 modal 配方） -->
    <div class="modal-backdrop" id="confirm-backdrop" hidden>
      <div class="modal" role="dialog" aria-modal="true" aria-label="删除笔记">
        <header class="modal-header">
          <h2 class="modal-title">删除笔记</h2>
        </header>
        <div class="modal-body">
          <p id="confirm-text">确定删除该笔记？此操作不可恢复。</p>
        </div>
        <footer class="modal-actions">
          <button class="btn-secondary" id="confirm-cancel" type="button">取消</button>
          <button class="btn-danger" id="confirm-delete" type="button">删除</button>
        </footer>
      </div>
    </div>
    <!-- M7：列表右键菜单 -->
    <div class="ctx-menu" id="ctx-menu" role="menu" aria-label="笔记操作" hidden>
      <button type="button" role="menuitem" id="ctx-delete">删除笔记</button>
    </div>
  `;
}

/** 重绘列表与编辑器（状态变更后调用）。 */
export function renderAll(): void {
  renderList();
  renderEditor();
}

/** M6：列表加载骨架屏（灰占位卡片，消除首次加载白屏期）。 */
export function renderSkeleton(count = 6): void {
  const list = document.getElementById("note-list");
  if (!list) return;
  list.innerHTML = Array.from(
    { length: count },
    () =>
      `<li class="skeleton-card" aria-hidden="true"><span class="sk sk-title"></span><span class="sk sk-preview"></span><span class="sk sk-meta"></span></li>`,
  ).join("");
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
  if (navView.get() === "trash") {
    renderTrashList(listEl, countEl);
    return;
  }
  const items = filteredNotes();
  countEl.textContent = String(items.length);

  if (items.length === 0) {
    listEl.innerHTML = emptyHtml(notes.get().length === 0);
    return;
  }
  const sel = selectedPath.get();
  listEl.innerHTML = items.map((n) => cardHtml(n, n.path === sel)).join("");
}

/** P0-数据安全：回收站列表（恢复 / 永久删除按钮，点击行不进入编辑器）。 */
function renderTrashList(listEl: HTMLElement, countEl: HTMLElement): void {
  const items = trash.get();
  countEl.textContent = String(items.length);
  if (items.length === 0) {
    listEl.innerHTML = `
      <div class="empty-state">
        <div class="empty-illus">🗑</div>
        <div class="empty-title">回收站是空的</div>
        <div class="empty-hint">删除的笔记会先到这里，可随时恢复</div>
      </div>`;
    return;
  }
  listEl.innerHTML =
    items.map(trashCardHtml).join("") +
    `<li class="trash-footer"><button type="button" class="btn-mini danger" id="trash-empty">清空回收站</button></li>`;
}

function trashCardHtml(t: TrashEntry): string {
  const title = t.title.trim() ? t.title : "（无标题）";
  return `
    <li class="note-card trash-card" data-trash-id="${escapeAttr(t.id)}" data-title="${escapeAttr(title)}" tabindex="0" role="group" aria-label="回收站条目 ${escapeAttr(title)}">
      <div class="card-top">
        <span class="card-title trash-title">${escapeHtml(title)}</span>
        <span class="badge badge-${t.format}">${t.format.toUpperCase()}</span>
      </div>
      <div class="card-meta">
        <span title="${escapeAttr(t.originalRel)}">${escapeHtml(t.originalRel)}</span>
        <span>· 删除于 ${formatTime(t.deletedAt)}</span>
        ${t.imageCount ? `<span>· ${t.imageCount} 图</span>` : ""}
      </div>
      <div class="trash-actions">
        <button type="button" class="btn-mini restore-btn" data-trash-id="${escapeAttr(t.id)}">恢复</button>
        <button type="button" class="btn-mini danger purge-btn" data-trash-id="${escapeAttr(t.id)}">永久删除</button>
      </div>
    </li>`;
}

function cardHtml(n: NoteMeta, selected: boolean): string {
  const title = n.title.trim() ? n.title : "（无标题）";
  const preview = n.preview.trim() ? n.preview : "空笔记";
  const tags = n.tags.length ? `<span class="card-tags">#${escapeHtml(n.tags.join(" #"))}</span>` : "";
  return `
    <li class="note-card${selected ? " selected" : ""}" data-path="${escapeAttr(n.path)}" tabindex="0" role="button" aria-label="${escapeAttr(n.title || "无标题")}">
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
  // P0-数据安全：回收站视图不加载编辑器（展示恢复/删除操作）
  if (navView.get() === "trash") {
    editor?.render(null);
    return;
  }
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
  const v = navView.get();
  $("#list-title").textContent = v === "pinned" ? "收藏" : v === "trash" ? "回收站" : "全部笔记";
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
