// 编辑器模块（M3）：CodeMirror 6 源码编辑 + markdown-it 预览 + 图文混排 + 自动保存。
// ui.ts 的 renderEditor() 调用 render()；main.ts 收到 notes://changed 后调用 onExternalChange()。

import { Compartment, EditorSelection, EditorState } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLineGutter,
  drawSelection,
  highlightActiveLine,
  dropCursor,
  placeholder as cmPlaceholder,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { bracketMatching, defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { markdown as cmMarkdown } from "@codemirror/lang-markdown";
import MarkdownIt from "markdown-it";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  acquireNoteLock,
  assetsImport,
  assetsImportBase64,
  convertFileSrc,
  noteAbsPath,
  readNote,
  releaseNoteLock,
  writeNote,
} from "./api";
import type { NoteMeta } from "../types";

type ViewMode = "edit" | "split" | "preview";
const MODE_KEY = "editor-mode";
const IMAGE_RE = /\.(png|jpe?g|gif|webp|bmp|svg)$/i;
/** 自动保存防抖（设计规范 §7：停顿 800ms 落盘）。 */
const SAVE_DELAY = 800;

// ---------- markdown-it ----------
// 安全验收（M3-2）：html:false 禁用 HTML 标签，`<script>` 等以文本呈现、不执行。
const md = new MarkdownIt({ html: false, linkify: true, breaks: true });
const origImageRule = md.renderer.rules.image;
/** 当前笔记所在目录（绝对路径，正斜杠），供图片 src 重写为 asset://。 */
let noteAbsDir = "";

md.renderer.rules.image = (tokens, idx, options, env, self) => {
  const token = tokens[idx];
  const src = token.attrGet("src"); // 类型：string | number | null
  if (src != null && noteAbsDir && !/^(?:data:|https?:|asset:|ftp:|#|\/)/i.test(String(src))) {
    // markdown-it 已对中文等字符做百分号编码，先解码再拼绝对路径，
    // 避免 convertFileSrc 二次编码导致路径错误。
    const decoded = safeDecode(String(src));
    token.attrSet("src", convertFileSrc(`${noteAbsDir}/${decoded}`));
  }
  return origImageRule
    ? origImageRule(tokens, idx, options, env, self)
    : self.renderToken(tokens, idx, options);
};

function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

// ---------- CodeMirror 分档：只读 / 语言 ----------
const readOnlyCompartment = new Compartment();
const languageCompartment = new Compartment();

interface OpenNote {
  rel: string;
  readOnly: boolean;
  dirty: boolean;
  lastSaved: string;
}

let container: HTMLElement | null = null;
let view: EditorView | null = null;
let open: OpenNote | null = null;
let mode: ViewMode = loadMode();
let saveTimer = 0;
let previewTimer = 0;
let loadToken = 0;
let dragUnlisten: (() => void) | null = null;

// 锁生命周期串行化（B1）：所有 acquire/release 沿同一条 promise 链执行，
// 确保「先释放旧锁、再获取新锁」有序落地，避免占位/重开后假锁冲突。
let lockChain: Promise<void> = Promise.resolve();
let lockedRel: string | null = null;

/** 串行获取笔记锁：先释放旧锁再获取新锁。返回是否成功（false = 被其他窗口占用）。 */
async function lockNote(rel: string): Promise<boolean> {
  const op = lockChain.then(async () => {
    if (lockedRel && lockedRel !== rel) {
      await releaseNoteLock(lockedRel).catch(() => {});
    }
    try {
      await acquireNoteLock(rel);
      lockedRel = rel;
      return true;
    } catch {
      return false;
    }
  });
  lockChain = op.then(() => {});
  return op;
}

/** 释放指定笔记的锁（幂等），并入锁链串行排队。 */
function releaseLockFor(rel: string): void {
  lockChain = lockChain.then(() => releaseNoteLock(rel).catch(() => {}));
  if (lockedRel === rel) lockedRel = null;
}

/** 释放当前持有的锁（若持有）。编辑器销毁/占位时调用。 */
function releaseOpenLock(): void {
  if (lockedRel) releaseLockFor(lockedRel);
}

// 点击工具栏外任意处关闭标题菜单（模块级注册一次）
document.addEventListener("click", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest(".heading-menu") && !t.closest("[data-cmd='heading']")) {
    closeHeadingMenu();
  }
});

// 关窗落盘（B2）：Tauri 环境拦截关窗请求 → 等待保存完成再真正销毁，
// 避免异步 IPC 尚未落地就关窗导致丢字。非 Tauri（纯 vite）退化为 beforeunload 尽力而为。
if (isTauri()) {
  getCurrentWindow()
    .onCloseRequested(async (event) => {
      event.preventDefault();
      clearTimeout(saveTimer);
      await flushSave();
      await getCurrentWindow().destroy().catch(() => {});
    })
    .catch(() => {
      /* 无窗口环境忽略 */
    });
}
window.addEventListener("beforeunload", () => {
  clearTimeout(saveTimer);
  void saveNow();
});

function loadMode(): ViewMode {
  const m = localStorage.getItem(MODE_KEY);
  return m === "edit" || m === "split" || m === "preview" ? m : "split";
}

// ============================================================
// 渲染入口
// ============================================================

/** ui.ts 入口：按当前选中笔记渲染编辑区（无选中 → 占位）。 */
export function render(target: HTMLElement, note: NoteMeta | null): void {
  if (!note) {
    showPlaceholder(target);
    return;
  }
  ensureShell(target);
  void loadNote(note);
}

function showPlaceholder(target: HTMLElement): void {
  const already = target.querySelector(".editor-placeholder") !== null;
  if (container !== target || !already) {
    destroyEditor();
    container = target;
    target.innerHTML = `
      <div class="editor-placeholder">
        <div class="ph-title">选择一篇笔记</div>
        <div class="ph-hint">从左侧列表选择开始阅读与编辑</div>
      </div>`;
  }
  loadToken++; // 使进行中的 loadNote 失效，避免写回已销毁的容器
  open = null;
  noteAbsDir = "";
}

function ensureShell(target: HTMLElement): void {
  if (target.querySelector(".editor-wrap")) {
    container = target;
    return;
  }
  destroyEditor();
  container = target;
  target.innerHTML = `
    <div class="editor-wrap">
      <header class="editor-header">
        <div class="editor-title-row">
          <button class="btn-icon back-btn" id="ed-back" title="返回列表" aria-label="返回列表">‹</button>
          <h1 class="editor-title" id="ed-title">…</h1>
          <span class="badge" id="ed-badge"></span>
        </div>
        <div class="toolbar" role="toolbar" aria-label="编辑器工具栏">
          <button class="tbtn" data-cmd="bold" title="粗体" aria-label="粗体">B</button>
          <button class="tbtn" data-cmd="italic" title="斜体" aria-label="斜体">I</button>
          <span class="tbtn-wrap">
            <button class="tbtn" data-cmd="heading" title="标题" aria-label="标题" aria-haspopup="true">H</button>
            <div class="heading-menu" id="heading-menu" hidden>
              <button data-h="1">H1 大标题</button>
              <button data-h="2">H2 中标题</button>
              <button data-h="3">H3 小标题</button>
            </div>
          </span>
          <button class="tbtn" data-cmd="code" title="代码块" aria-label="代码块">⟨/⟩</button>
          <button class="tbtn" data-cmd="image" title="插入图片" aria-label="插入图片">▦</button>
          <span class="toolbar-sep"></span>
          <button class="tbtn" data-cmd="export" title="复制 Markdown" aria-label="复制 Markdown">⧉</button>
          <span class="toolbar-grow"></span>
          <button class="tbtn mode-btn${mode === "edit" ? " active" : ""}" data-mode="edit">编辑</button>
          <button class="tbtn mode-btn${mode === "split" ? " active" : ""}" data-mode="split">双栏</button>
          <button class="tbtn mode-btn${mode === "preview" ? " active" : ""}" data-mode="preview">预览</button>
        </div>
      </header>
      <div class="editor-body ${mode === "split" ? "split" : mode === "preview" ? "preview" : "edit"}" id="ed-body">
        <div class="cm-host" id="ed-cm"></div>
        <div class="preview-pane" id="ed-preview" aria-label="预览"></div>
      </div>
      <footer class="editor-status" id="ed-status">
        <span id="ed-wc"></span>
        <span class="status-sep">·</span>
        <span id="ed-format"></span>
        <span class="status-grow"></span>
        <span id="ed-save" class="save-state">未编辑</span>
      </footer>
    </div>`;
  bindShell();
}

function destroyEditor(): void {
  view?.destroy();
  view = null;
  if (dragUnlisten) {
    dragUnlisten();
    dragUnlisten = null;
  }
  releaseOpenLock(); // 归还当前笔记的锁，避免占位/重开时假只读（B1）
  open = null;
  noteAbsDir = "";
}

/** 外部（磁盘）改动后的处理：本地无未保存内容时重载，避免覆盖用户输入。 */
export async function onExternalChange(): Promise<void> {
  const n = open;
  if (!n || !view || n.dirty) return;
  try {
    const disk = await readNote(n.rel);
    const head = view.state.selection.main.head;
    if (disk === view.state.doc.toString()) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: disk },
      selection: { anchor: Math.min(head, disk.length) },
    });
    n.lastSaved = disk;
    n.dirty = false;
    setSave("已从磁盘刷新", "saved");
    updateStatus();
    renderPreview();
  } catch {
    /* 笔记可能已被删除，忽略 */
  }
}

// ============================================================
// 笔记载入
// ============================================================

async function loadNote(note: NoteMeta): Promise<void> {
  if (open?.rel === note.path) return;
  const token = ++loadToken;

  await flushSave();
  open = null; // 旧笔记已落盘；其锁的释放由 lockNote 在获取新锁前串行接管（B1）

  let content = "";
  try {
    content = await readNote(note.path);
  } catch (err) {
    setSave(errText(err), "error");
    return;
  }
  if (token !== loadToken) return;

  try {
    const abs = await noteAbsPath(note.path);
    noteAbsDir = abs.replace(/\\/g, "/").replace(/\/[^/]*$/, "");
  } catch {
    noteAbsDir = "";
  }

  const readOnly = !(await lockNote(note.path));
  if (readOnly) setSave("该笔记正被其他窗口编辑，已以只读打开", "warn");
  if (token !== loadToken) {
    // 快速切换期间本次获取已失效；归还已拿到的锁（幂等，不影响当前笔记）
    if (!readOnly) releaseLockFor(note.path);
    return;
  }

  open = { rel: note.path, readOnly, dirty: false, lastSaved: content };

  const titleEl = container?.querySelector<HTMLElement>("#ed-title");
  if (titleEl) titleEl.textContent = note.title.trim() ? note.title : "（无标题）";
  const badgeEl = container?.querySelector<HTMLElement>("#ed-badge");
  if (badgeEl) {
    badgeEl.textContent = note.format.toUpperCase();
    badgeEl.className = `badge badge-${note.format}`;
  }

  const v = ensureView(content, readOnly);
  v.dispatch({
    changes: { from: 0, to: v.state.doc.length, insert: content },
    effects: [
      languageCompartment.reconfigure(note.format === "md" ? [cmMarkdown()] : []),
      readOnlyCompartment.reconfigure(EditorState.readOnly.of(readOnly)),
    ],
    selection: { anchor: 0 },
  });
  open.dirty = false;
  open.lastSaved = content;
  updateStatus();
  renderPreview();
}

function ensureView(doc: string, readOnly: boolean): EditorView {
  if (view) return view;
  const host = container?.querySelector<HTMLElement>("#ed-cm");
  if (!host) throw new Error("编辑器容器缺失");
  view = new EditorView({ state: createState(doc, readOnly), parent: host });
  return view;
}

function createState(doc: string, readOnly: boolean): EditorState {
  const dark = document.documentElement.getAttribute("data-theme") === "dark";
  return EditorState.create({
    doc,
    extensions: [
      lineNumbers(),
      highlightActiveLineGutter(),
      history(),
      languageCompartment.of([]),
      bracketMatching(),
      highlightActiveLine(),
      drawSelection(),
      dropCursor(),
      cmPlaceholder("开始输入…"),
      readOnlyCompartment.of(EditorState.readOnly.of(readOnly)),
      syntaxHighlighting(defaultHighlightStyle),
      keymap.of([
        ...defaultKeymap,
        ...historyKeymap,
        indentWithTab,
        { key: "Mod-s", run: () => { void saveNow(); return true; } },
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          if (open) open.dirty = true;
          updateStatus();
          schedulePreview();
          scheduleSave();
        }
      }),
      EditorView.theme(
        {
          "&": {
            height: "100%",
            fontSize: "var(--fs-body)",
            color: "var(--ink)",
            backgroundColor: "var(--bg)",
          },
          "&.cm-focused": { outline: "none" },
          ".cm-scroller": { fontFamily: "var(--font-zh)", lineHeight: "var(--lh-body)", overflow: "auto" },
          ".cm-content": { padding: "var(--space-4) var(--space-5)", caretColor: "var(--accent)" },
          ".cm-line": { padding: "0 2px" },
          ".cm-gutters": {
            backgroundColor: "var(--surface)",
            color: "var(--ink-2)",
            borderRight: "1px solid var(--border)",
          },
          ".cm-activeLine": { backgroundColor: "var(--mark)" },
          ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--accent)" },
          ".cm-cursor": { borderLeftColor: "var(--accent)" },
        },
        { dark },
      ),
    ],
  });
}

// ============================================================
// 自动保存（800ms 防抖 + 原子写）
// ============================================================

async function saveNow(): Promise<void> {
  const n = open;
  if (!n || !view || n.readOnly || !n.dirty) return;
  const content = view.state.doc.toString();
  if (content === n.lastSaved) {
    n.dirty = false;
    return;
  }
  try {
    await writeNote(n.rel, content);
    n.lastSaved = content;
    n.dirty = false;
    setSave("已自动保存", "saved");
    // B7：自身写入不再触发 watcher 全量重扫；通知主进程本地更新 mtime 重排
    window.dispatchEvent(new CustomEvent("note:saved", { detail: { rel: n.rel } }));
  } catch (err) {
    setSave(errText(err), "error");
  }
}

function scheduleSave(): void {
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void saveNow(), SAVE_DELAY);
}

async function flushSave(): Promise<void> {
  clearTimeout(saveTimer);
  await saveNow();
}

// ============================================================
// 预览
// ============================================================

function renderPreview(): void {
  const pane = container?.querySelector<HTMLElement>("#ed-preview");
  if (!pane) return;
  if (mode === "edit") {
    pane.innerHTML = "";
    return;
  }
  const text = view?.state.doc.toString() ?? "";
  pane.innerHTML = md.render(text);
}

function schedulePreview(): void {
  clearTimeout(previewTimer);
  previewTimer = window.setTimeout(renderPreview, 120);
}

// ============================================================
// 工具栏
// ============================================================

function bindShell(): void {
  container?.querySelector<HTMLElement>(".toolbar")?.addEventListener("click", onToolbarClick);
  container?.querySelectorAll<HTMLElement>("[data-mode]").forEach((btn) => {
    btn.addEventListener("click", () => setMode(btn.dataset.mode as ViewMode));
  });
  container?.querySelector<HTMLElement>("#ed-back")?.addEventListener("click", () => {
    window.dispatchEvent(new CustomEvent("editor:back"));
  });
  const preview = container?.querySelector<HTMLElement>("#ed-preview");
  preview?.addEventListener("click", (e) => {
    const a = (e.target as HTMLElement).closest<HTMLAnchorElement>("a");
    if (!a) return;
    // 所有链接一律拦截默认导航（S1）：仅 http(s)/mailto 交由系统打开。
    // 其余 scheme（javascript:/file:/data:/asset:）与相对路径不响应——
    // javascript: 会在应用上下文执行脚本（恶意笔记 → 完整 IPC 权限），
    // 相对/asset 链接会整页导航破坏 SPA。检查原始 href 属性，而非浏览器解析后的 a.href。
    e.preventDefault();
    const href = a.getAttribute("href") ?? "";
    if (/^(?:https?:|mailto:)/i.test(href)) {
      void openUrl(href).catch(() => {});
    }
  });
  // 图片加载失败诊断：error 不冒泡，用捕获阶段监听
  preview?.addEventListener(
    "error",
    (e) => {
      const img = e.target as HTMLImageElement;
      if (img?.src) setSave(`图片加载失败：${img.src.slice(0, 160)}`, "error");
    },
    true,
  );
  // 图片拖入（Tauri 环境）。
  if (isTauri()) {
    getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "drop") void onDrop(e.payload.paths);
      })
      .then((un) => {
        // 若期间已切回占位（壳已销毁），立即退订，避免残留监听
        if (container?.querySelector(".editor-wrap")) dragUnlisten = un;
        else un();
      })
      .catch(() => {});
  }
  // 仅文件拖入时阻止默认，避免 WebView 直接打开文件；文本拖拽仍可用。
  const hasFiles = (e: DragEvent) =>
    e.dataTransfer ? Array.from(e.dataTransfer.types).includes("Files") : false;
  container?.addEventListener("dragover", (e) => {
    if (hasFiles(e)) e.preventDefault();
  });
  container?.addEventListener("drop", (e) => {
    if (hasFiles(e)) e.preventDefault();
  });
}

function onToolbarClick(e: MouseEvent): void {
  const target = e.target as HTMLElement;
  const hBtn = target.closest<HTMLElement>("[data-h]");
  if (hBtn) {
    setHeading(Number(hBtn.dataset.h));
    closeHeadingMenu();
    return;
  }
  const btn = target.closest<HTMLElement>("[data-cmd]");
  if (!btn) return;
  const cmd = btn.dataset.cmd;
  if (cmd === "heading") {
    const menu = container?.querySelector<HTMLElement>("#heading-menu");
    if (menu) {
      const willOpen = menu.hidden;
      menu.hidden = !willOpen;
      if (willOpen) {
        const rect = btn.getBoundingClientRect();
        menu.style.top = `${rect.bottom + 4}px`;
        menu.style.left = `${rect.left}px`;
      }
    }
  } else if (cmd === "image") {
    pickImage();
  } else if (cmd === "bold") {
    wrapSelection("**", "**");
  } else if (cmd === "italic") {
    wrapSelection("*", "*");
  } else if (cmd === "code") {
    insertCodeBlock();
  } else if (cmd === "export") {
    void exportMarkdown();
  }
}

function closeHeadingMenu(): void {
  const menu = container?.querySelector<HTMLElement>("#heading-menu");
  if (menu) menu.hidden = true;
}

function setMode(next: ViewMode): void {
  mode = next;
  localStorage.setItem(MODE_KEY, next);
  const body = container?.querySelector<HTMLElement>("#ed-body");
  if (body) {
    body.classList.remove("edit", "split", "preview");
    body.classList.add(next);
  }
  container?.querySelectorAll<HTMLElement>(".mode-btn").forEach((b) => {
    b.classList.toggle("active", b.dataset.mode === next);
  });
  if (next !== "edit") renderPreview();
}

function wrapSelection(before: string, after: string): void {
  if (!view) return;
  const sel = view.state.selection.main;
  const selected = view.state.sliceDoc(sel.from, sel.to);
  // 无选中：插入 `**` + `**` 并把光标放到中间，用户直接输入即被包住
  const text = sel.empty ? before + after : before + selected + after;
  const cursor =
    sel.from + before.length + (sel.empty ? 0 : selected.length);
  view.dispatch({
    changes: { from: sel.from, to: sel.to, insert: text },
    selection: EditorSelection.cursor(cursor),
    userEvent: "input",
  });
  view.focus();
}

function insertCodeBlock(): void {
  if (!view) return;
  const sel = view.state.selection.main;
  const selected = sel.empty ? "" : view.state.sliceDoc(sel.from, sel.to);
  const insert = selected ? "```\n" + selected + "\n```" : "```\n\n```";
  view.dispatch({
    changes: { from: sel.from, to: sel.to, insert },
    selection: EditorSelection.cursor(sel.from + insert.length),
    userEvent: "input",
  });
  view.focus();
}

function setHeading(level: number): void {
  if (!view) return;
  const pos = view.state.selection.main.head;
  const line = view.state.doc.lineAt(pos);
  const m = line.text.match(/^(#{1,6})\s/);
  const prefix = "#".repeat(level) + " ";
  let newText: string;
  let cursorFrom: number;
  if (m && m[1].length === level) {
    newText = line.text.replace(/^#{1,6}\s/, "");
    cursorFrom = line.from;
  } else {
    newText = prefix + line.text.replace(/^#{1,6}\s/, "");
    cursorFrom = line.from + prefix.length;
  }
  view.dispatch({
    changes: { from: line.from, to: line.to, insert: newText },
    selection: EditorSelection.cursor(cursorFrom),
    userEvent: "input",
  });
  view.focus();
}

async function exportMarkdown(): Promise<void> {
  const text = view?.state.doc.toString() ?? "";
  try {
    await navigator.clipboard.writeText(text);
    setSave("已复制 Markdown 到剪贴板", "saved");
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand("copy");
      setSave("已复制 Markdown 到剪贴板", "saved");
    } catch {
      setSave("复制失败", "error");
    }
    ta.remove();
  }
}

// ============================================================
// 图片导入
// ============================================================

function pickImage(): void {
  const n = open;
  if (!n || n.readOnly) return;
  const input = document.createElement("input");
  input.type = "file";
  input.accept = "image/*";
  input.onchange = () => {
    const file = input.files?.[0];
    input.remove();
    if (file) void importPicked(file, n.rel);
  };
  input.click();
}

const MAX_IMAGE_BYTES = 20 * 1024 * 1024;

async function importPicked(file: File, noteRel: string): Promise<void> {
  if (file.size > MAX_IMAGE_BYTES) {
    setSave(`图片超过 20MB 上限：${file.name}`, "error");
    return;
  }
  try {
    const buf = await file.arrayBuffer();
    const res = await assetsImportBase64(noteRel, file.name, bytesToBase64(new Uint8Array(buf)));
    insertImageRef(relToNote(noteRel, res.rel), baseName(file.name));
    setSave(`已导入 ${file.name}`, "saved");
  } catch (err) {
    setSave(errText(err), "error");
  }
}

async function onDrop(paths: string[]): Promise<void> {
  const n = open;
  if (!n || n.readOnly) return;
  const images = paths.filter((p) => IMAGE_RE.test(p));
  if (images.length === 0) {
    setSave("仅支持图片文件（png/jpg/gif/webp/bmp/svg）", "warn");
    return;
  }
  let ok = 0;
  for (const src of images) {
    try {
      const res = await assetsImport(n.rel, src);
      insertImageRef(relToNote(n.rel, res.rel), baseName(src));
      ok++;
    } catch (err) {
      setSave(errText(err), "error");
    }
  }
  if (ok) setSave(`已导入 ${ok} 张图片`, "saved");
}

function insertImageRef(rel: string, name: string): void {
  if (!view) return;
  const sel = view.state.selection.main;
  const text = `![${name}](${rel})`;
  view.dispatch({
    changes: { from: sel.from, to: sel.to, insert: text },
    selection: EditorSelection.cursor(sel.from + text.length),
    userEvent: "input",
  });
  view.focus();
}

/** 把根内图片相对引用换算成「相对笔记文件」的引用（同一目录内用短路径）。 */
function relToNote(noteRel: string, assetRel: string): string {
  const idx = noteRel.lastIndexOf("/");
  const dirRel = idx >= 0 ? noteRel.slice(0, idx + 1) : "";
  return assetRel.startsWith(dirRel) ? assetRel.slice(dirRel.length) : assetRel;
}

function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}

function baseName(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  const name = i >= 0 ? p.slice(i + 1) : p;
  return (name.replace(/\.[^.]+$/, "").slice(0, 32) || "图片");
}

// ============================================================
// 状态栏
// ============================================================

function updateStatus(): void {
  const wc = container?.querySelector<HTMLElement>("#ed-wc");
  const fmt = container?.querySelector<HTMLElement>("#ed-format");
  if (!wc || !fmt) return;
  const text = view?.state.doc.toString() ?? "";
  wc.textContent = `字数 ${text.replace(/\s/g, "").length}`;
  fmt.textContent = open
    ? open.rel.toLowerCase().endsWith(".md")
      ? "Markdown"
      : "纯文本"
    : "";
}

function setSave(text: string, cls: "" | "saved" | "warn" | "error"): void {
  const el = container?.querySelector<HTMLElement>("#ed-save");
  if (!el) return;
  el.textContent = text;
  el.className = "save-state" + (cls ? ` ${cls}` : "");
}

function errText(err: unknown): string {
  return String(err).replace(/^Error:\s*/, "");
}
