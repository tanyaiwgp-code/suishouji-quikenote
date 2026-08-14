// 编辑器模块（M3/M4）：CodeMirror 6 源码编辑 + markdown-it 预览 + 图文混排 + 自动保存。
// 工厂化（M4 前置）：createEditor(target) 返回独立编辑器实例——主窗口与快速记录浮窗各持一个。
// 窗口生命周期（onCloseRequested / beforeunload → flushSave）由各窗口入口（main.ts）接线，本模块导入零副作用。

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
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  acquireNoteLock,
  ApiError,
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
/** 单张图片导入上限（M3：assets_import 同款校验）。 */
const MAX_IMAGE_BYTES = 20 * 1024 * 1024;

function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

interface OpenNote {
  rel: string;
  readOnly: boolean;
  dirty: boolean;
  lastSaved: string;
}

function loadMode(): ViewMode {
  const m = localStorage.getItem(MODE_KEY);
  return m === "edit" || m === "split" || m === "preview" ? m : "split";
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

function errText(err: unknown): string {
  // ApiError 直接取 message（中文文案，无 "Error: " 前缀）；其余按字符串降级。
  if (err instanceof ApiError) return err.message;
  return String(err).replace(/^Error:\s*/, "");
}

/** 编辑器实例的公开接口：target 在构造时绑定，实例间状态完全隔离。 */
export interface EditorInstance {
  /** 按当前选中笔记渲染编辑区（无选中 → 占位）。 */
  render(note: NoteMeta | null): void;
  /** 外部（磁盘）改动后的处理：本地无未保存内容时重载，避免覆盖用户输入。 */
  onExternalChange(): Promise<void>;
  /** 立即落盘（清防抖定时器 + 保存未保存内容）。关窗前调用。 */
  flushSave(): Promise<void>;
  /** 返回当前编辑内容（未载入笔记时为空字符串）。快速记录浮窗 finalize 用。 */
  getContent(): string;
  /** 聚焦 CodeMirror 正文。 */
  focus(): void;
  /** 销毁实例：清定时器、退监听、归还文件锁、清空容器。幂等，destroy 后可再 render。 */
  destroy(): void;
}

/** M4：编辑器外壳选项，全部默认 true；快速记录浮窗关闭回退按钮/模式按钮/状态栏/标题徽章。 */
export interface EditorOptions {
  backButton?: boolean;
  modeButtons?: boolean;
  statusBar?: boolean;
  titleBadge?: boolean;
}

/** 创建一个绑定到 target 容器的编辑器实例。 */
export function createEditor(target: HTMLElement, opts: EditorOptions = {}): EditorInstance {
  return new Editor(target, opts);
}

class Editor implements EditorInstance {
  // markdown-it 实例 per-instance（其 image 规则闭包引用本实例的 noteAbsDir）。
  // 安全验收（M3-2）：html:false 禁用 HTML 标签，`<script>` 等以文本呈现、不执行。
  private md = new MarkdownIt({ html: false, linkify: true, breaks: true });
  /** 当前笔记所在目录（绝对路径，正斜杠），供图片 src 重写为 asset://。 */
  private noteAbsDir = "";

  // CodeMirror 分档：只读 / 语言。必须 per-instance，多实例各自重配互不串扰。
  private readOnlyCompartment = new Compartment();
  private languageCompartment = new Compartment();

  /** 构造时绑定的宿主容器，不可变。 */
  private readonly host: HTMLElement;
  /** 当前工作容器；destroy() 置 null，render() 时经 host 重挂。 */
  private container: HTMLElement | null;
  /** 外壳配置（M4 精简壳用），构造时合并默认值。 */
  private readonly opts: Required<EditorOptions>;

  private view: EditorView | null = null;
  private open: OpenNote | null = null;
  private mode: ViewMode;
  private saveTimer = 0;
  private previewTimer = 0;
  private loadToken = 0;
  private dragUnlisten: (() => void) | null = null;

  // 锁生命周期串行化（B1）：所有 acquire/release 沿同一条 promise 链执行，
  // 确保「先释放旧锁、再获取新锁」有序落地，避免占位/重开后假锁冲突。
  private lockChain: Promise<void> = Promise.resolve();
  private lockedRel: string | null = null;

  private onDocClickBound: (e: MouseEvent) => void;

  constructor(target: HTMLElement, opts: EditorOptions = {}) {
    this.host = target;
    this.container = target;
    this.opts = { backButton: true, modeButtons: true, statusBar: true, titleBadge: true, ...opts };
    this.mode = this.opts.modeButtons ? loadMode() : "edit"; // 无模式按钮时钉死编辑态

    const origImageRule = this.md.renderer.rules.image;
    this.md.renderer.rules.image = (tokens, idx, options, env, self) => {
      const token = tokens[idx];
      const src = token.attrGet("src"); // 类型：string | number | null
      if (src != null && this.noteAbsDir && !/^(?:data:|https?:|asset:|ftp:|#|\/)/i.test(String(src))) {
        // markdown-it 已对中文等字符做百分号编码，先解码再拼绝对路径，
        // 避免 convertFileSrc 二次编码导致路径错误。
        const decoded = safeDecode(String(src));
        token.attrSet("src", convertFileSrc(`${this.noteAbsDir}/${decoded}`));
      }
      return origImageRule
        ? origImageRule(tokens, idx, options, env, self)
        : self.renderToken(tokens, idx, options);
    };

    // 点击工具栏外任意处关闭标题菜单（per-instance，只关自己的菜单；destroy 时移除）
    this.onDocClickBound = (e) => {
      const t = e.target as HTMLElement;
      if (!t.closest(".heading-menu") && !t.closest("[data-cmd='heading']")) {
        this.closeHeadingMenu();
      }
    };
    document.addEventListener("click", this.onDocClickBound);
  }

  // ============================================================
  // 渲染入口
  // ============================================================

  /** 按当前选中笔记渲染编辑区（无选中 → 占位）。 */
  render(note: NoteMeta | null): void {
    if (!this.container) this.container = this.host; // destroy() 后重挂
    if (!note) {
      this.showPlaceholder();
      return;
    }
    this.ensureShell();
    void this.loadNote(note);
  }

  private showPlaceholder(): void {
    const target = this.host;
    const already = target.querySelector(".editor-placeholder") !== null;
    if (!already) {
      this.destroyEditor();
      this.container = target;
      target.innerHTML = `
        <div class="editor-placeholder">
          <div class="ph-title">选择一篇笔记</div>
          <div class="ph-hint">从左侧列表选择开始阅读与编辑</div>
        </div>`;
    }
    this.loadToken++; // 使进行中的 loadNote 失效，避免写回已销毁的容器
    this.open = null;
    this.noteAbsDir = "";
  }

  private ensureShell(): void {
    const target = this.host;
    if (target.querySelector(".editor-wrap")) {
      this.container = target;
      return;
    }
    this.destroyEditor();
    this.container = target;
    const o = this.opts;
    target.innerHTML = `
      <div class="editor-wrap">
        <header class="editor-header">
          ${o.titleBadge ? `
          <div class="editor-title-row">
            ${o.backButton ? `<button class="btn-icon back-btn" id="ed-back" title="返回列表" aria-label="返回列表">‹</button>` : ""}
            <h1 class="editor-title" id="ed-title">…</h1>
            <span class="badge" id="ed-badge"></span>
          </div>` : ""}
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
            ${o.modeButtons ? `
            <button class="tbtn mode-btn${this.mode === "edit" ? " active" : ""}" data-mode="edit">编辑</button>
            <button class="tbtn mode-btn${this.mode === "split" ? " active" : ""}" data-mode="split">双栏</button>
            <button class="tbtn mode-btn${this.mode === "preview" ? " active" : ""}" data-mode="preview">预览</button>` : ""}
          </div>
        </header>
        <div class="editor-body ${this.mode === "split" ? "split" : this.mode === "preview" ? "preview" : "edit"}" id="ed-body">
          <div class="cm-host" id="ed-cm"></div>
          <div class="preview-pane" id="ed-preview" aria-label="预览"></div>
        </div>
        ${o.statusBar ? `
        <footer class="editor-status" id="ed-status">
          <span id="ed-wc"></span>
          <span class="status-sep">·</span>
          <span id="ed-format"></span>
          <span class="status-grow"></span>
          <span id="ed-save" class="save-state">未编辑</span>
        </footer>` : ""}
      </div>`;
    this.bindShell();
  }

  /** 内部销毁：释放 view/拖拽监听/文件锁。不清定时器、不重置 container（公有 destroy 补）。 */
  private destroyEditor(): void {
    this.view?.destroy();
    this.view = null;
    if (this.dragUnlisten) {
      this.dragUnlisten();
      this.dragUnlisten = null;
    }
    this.releaseOpenLock(); // 归还当前笔记的锁，避免占位/重开时假只读（B1）
    this.open = null;
    this.noteAbsDir = "";
  }

  /** 外部（磁盘）改动后的处理：本地无未保存内容时重载，避免覆盖用户输入。 */
  async onExternalChange(): Promise<void> {
    const n = this.open;
    if (!n || !this.view || n.dirty) return;
    try {
      const disk = await readNote(n.rel);
      const head = this.view.state.selection.main.head;
      if (disk === this.view.state.doc.toString()) return;
      this.view.dispatch({
        changes: { from: 0, to: this.view.state.doc.length, insert: disk },
        selection: { anchor: Math.min(head, disk.length) },
      });
      n.lastSaved = disk;
      n.dirty = false;
      this.setSave("已从磁盘刷新", "saved");
      this.updateStatus();
      this.renderPreview();
    } catch {
      /* 笔记可能已被删除，忽略 */
    }
  }

  // ============================================================
  // 自动保存（800ms 防抖 + 原子写）
  // ============================================================

  private async saveNow(): Promise<void> {
    const n = this.open;
    if (!n || !this.view || n.readOnly || !n.dirty) return;
    const content = this.view.state.doc.toString();
    if (content === n.lastSaved) {
      n.dirty = false;
      return;
    }
    try {
      await writeNote(n.rel, content);
      n.lastSaved = content;
      n.dirty = false;
      this.setSave("已自动保存", "saved");
      // B7：自身写入不再触发 watcher 全量重扫；通知主进程本地更新 mtime 重排
      window.dispatchEvent(new CustomEvent("note:saved", { detail: { rel: n.rel } }));
    } catch (err) {
      this.setSave(errText(err), "error");
    }
  }

  private scheduleSave(): void {
    clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => void this.saveNow(), SAVE_DELAY);
  }

  /** 立即落盘（关窗前调用，B2）。 */
  async flushSave(): Promise<void> {
    clearTimeout(this.saveTimer);
    await this.saveNow();
  }

  /** 当前编辑内容（未载入笔记时为空字符串）。快速记录浮窗 finalize 用。 */
  getContent(): string {
    return this.view?.state.doc.toString() ?? "";
  }

  /** 聚焦 CodeMirror 正文。 */
  focus(): void {
    this.view?.focus();
  }

  // ============================================================
  // 预览
  // ============================================================

  private renderPreview(): void {
    const pane = this.container?.querySelector<HTMLElement>("#ed-preview");
    if (!pane) return;
    if (this.mode === "edit") {
      pane.innerHTML = "";
      return;
    }
    const text = this.view?.state.doc.toString() ?? "";
    pane.innerHTML = this.md.render(text);
  }

  private schedulePreview(): void {
    clearTimeout(this.previewTimer);
    this.previewTimer = window.setTimeout(() => this.renderPreview(), 120);
  }

  // ============================================================
  // 工具栏
  // ============================================================

  private bindShell(): void {
    // onToolbarClick 为方法，DOM 监听器内 this 指向目标元素，需包箭头绑定实例
    this.container
      ?.querySelector<HTMLElement>(".toolbar")
      ?.addEventListener("click", (e) => this.onToolbarClick(e));
    this.container?.querySelectorAll<HTMLElement>("[data-mode]").forEach((btn) => {
      btn.addEventListener("click", () => this.setMode(btn.dataset.mode as ViewMode));
    });
    this.container?.querySelector<HTMLElement>("#ed-back")?.addEventListener("click", () => {
      window.dispatchEvent(new CustomEvent("editor:back"));
    });
    const preview = this.container?.querySelector<HTMLElement>("#ed-preview");
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
        if (img?.src) this.setSave(`图片加载失败：${img.src.slice(0, 160)}`, "error");
      },
      true,
    );
    // 图片拖入（Tauri 环境）。
    if (isTauri()) {
      getCurrentWebview()
        .onDragDropEvent((e) => {
          if (e.payload.type === "drop") void this.onDrop(e.payload.paths);
        })
        .then((un) => {
          // 若期间已切回占位（壳已销毁），立即退订，避免残留监听
          if (this.container?.querySelector(".editor-wrap")) this.dragUnlisten = un;
          else un();
        })
        .catch(() => {});
    }
    // 仅文件拖入时阻止默认，避免 WebView 直接打开文件；文本拖拽仍可用。
    const hasFiles = (e: DragEvent) =>
      e.dataTransfer ? Array.from(e.dataTransfer.types).includes("Files") : false;
    this.container?.addEventListener("dragover", (e) => {
      if (hasFiles(e)) e.preventDefault();
    });
    this.container?.addEventListener("drop", (e) => {
      if (hasFiles(e)) e.preventDefault();
    });
  }

  private onToolbarClick(e: MouseEvent): void {
    const target = e.target as HTMLElement;
    const hBtn = target.closest<HTMLElement>("[data-h]");
    if (hBtn) {
      this.setHeading(Number(hBtn.dataset.h));
      this.closeHeadingMenu();
      return;
    }
    const btn = target.closest<HTMLElement>("[data-cmd]");
    if (!btn) return;
    const cmd = btn.dataset.cmd;
    if (cmd === "heading") {
      const menu = this.container?.querySelector<HTMLElement>("#heading-menu");
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
      this.pickImage();
    } else if (cmd === "bold") {
      this.wrapSelection("**", "**");
    } else if (cmd === "italic") {
      this.wrapSelection("*", "*");
    } else if (cmd === "code") {
      this.insertCodeBlock();
    } else if (cmd === "export") {
      void this.exportMarkdown();
    }
  }

  private closeHeadingMenu(): void {
    const menu = this.container?.querySelector<HTMLElement>("#heading-menu");
    if (menu) menu.hidden = true;
  }

  private setMode(next: ViewMode): void {
    this.mode = next;
    localStorage.setItem(MODE_KEY, next);
    const body = this.container?.querySelector<HTMLElement>("#ed-body");
    if (body) {
      body.classList.remove("edit", "split", "preview");
      body.classList.add(next);
    }
    this.container?.querySelectorAll<HTMLElement>(".mode-btn").forEach((b) => {
      b.classList.toggle("active", b.dataset.mode === next);
    });
    if (next !== "edit") this.renderPreview();
  }

  private wrapSelection(before: string, after: string): void {
    if (!this.view) return;
    const sel = this.view.state.selection.main;
    const selected = this.view.state.sliceDoc(sel.from, sel.to);
    // 无选中：插入 `**` + `**` 并把光标放到中间，用户直接输入即被包住
    const text = sel.empty ? before + after : before + selected + after;
    const cursor =
      sel.from + before.length + (sel.empty ? 0 : selected.length);
    this.view.dispatch({
      changes: { from: sel.from, to: sel.to, insert: text },
      selection: EditorSelection.cursor(cursor),
      userEvent: "input",
    });
    this.view.focus();
  }

  private insertCodeBlock(): void {
    if (!this.view) return;
    const sel = this.view.state.selection.main;
    const selected = sel.empty ? "" : this.view.state.sliceDoc(sel.from, sel.to);
    const insert = selected ? "```\n" + selected + "\n```" : "```\n\n```";
    this.view.dispatch({
      changes: { from: sel.from, to: sel.to, insert },
      selection: EditorSelection.cursor(sel.from + insert.length),
      userEvent: "input",
    });
    this.view.focus();
  }

  private setHeading(level: number): void {
    if (!this.view) return;
    const pos = this.view.state.selection.main.head;
    const line = this.view.state.doc.lineAt(pos);
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
    this.view.dispatch({
      changes: { from: line.from, to: line.to, insert: newText },
      selection: EditorSelection.cursor(cursorFrom),
      userEvent: "input",
    });
    this.view.focus();
  }

  private async exportMarkdown(): Promise<void> {
    const text = this.view?.state.doc.toString() ?? "";
    try {
      await navigator.clipboard.writeText(text);
      this.setSave("已复制 Markdown 到剪贴板", "saved");
    } catch {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.opacity = "0";
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand("copy");
        this.setSave("已复制 Markdown 到剪贴板", "saved");
      } catch {
        this.setSave("复制失败", "error");
      }
      ta.remove();
    }
  }

  // ============================================================
  // 笔记载入
  // ============================================================

  private async loadNote(note: NoteMeta): Promise<void> {
    if (this.open?.rel === note.path) return;
    const token = ++this.loadToken;

    await this.flushSave();
    this.open = null; // 旧笔记已落盘；其锁的释放由 lockNote 在获取新锁前串行接管（B1）

    let content = "";
    try {
      content = await readNote(note.path);
    } catch (err) {
      this.setSave(errText(err), "error");
      return;
    }
    if (token !== this.loadToken) return;

    try {
      const abs = await noteAbsPath(note.path);
      this.noteAbsDir = abs.replace(/\\/g, "/").replace(/\/[^/]*$/, "");
    } catch {
      this.noteAbsDir = "";
    }

    const readOnly = !(await this.lockNote(note.path));
    if (readOnly) this.setSave("该笔记正被其他窗口编辑，已以只读打开", "warn");
    if (token !== this.loadToken) {
      // 快速切换期间本次获取已失效；归还已拿到的锁（幂等，不影响当前笔记）
      if (!readOnly) this.releaseLockFor(note.path);
      return;
    }

    this.open = { rel: note.path, readOnly, dirty: false, lastSaved: content };

    const titleEl = this.container?.querySelector<HTMLElement>("#ed-title");
    if (titleEl) titleEl.textContent = note.title.trim() ? note.title : "（无标题）";
    const badgeEl = this.container?.querySelector<HTMLElement>("#ed-badge");
    if (badgeEl) {
      badgeEl.textContent = note.format.toUpperCase();
      badgeEl.className = `badge badge-${note.format}`;
    }

    const v = this.ensureView(content, readOnly);
    v.dispatch({
      changes: { from: 0, to: v.state.doc.length, insert: content },
      effects: [
        this.languageCompartment.reconfigure(note.format === "md" ? [cmMarkdown()] : []),
        this.readOnlyCompartment.reconfigure(EditorState.readOnly.of(readOnly)),
      ],
      selection: { anchor: 0 },
    });
    this.open.dirty = false;
    this.open.lastSaved = content;
    this.updateStatus();
    this.renderPreview();
  }

  private ensureView(doc: string, readOnly: boolean): EditorView {
    if (this.view) return this.view;
    const host = this.container?.querySelector<HTMLElement>("#ed-cm");
    if (!host) throw new Error("编辑器容器缺失");
    this.view = new EditorView({ state: this.createState(doc, readOnly), parent: host });
    return this.view;
  }

  private createState(doc: string, readOnly: boolean): EditorState {
    const dark = document.documentElement.getAttribute("data-theme") === "dark";
    return EditorState.create({
      doc,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        history(),
        this.languageCompartment.of([]),
        bracketMatching(),
        highlightActiveLine(),
        drawSelection(),
        dropCursor(),
        cmPlaceholder("开始输入…"),
        this.readOnlyCompartment.of(EditorState.readOnly.of(readOnly)),
        syntaxHighlighting(defaultHighlightStyle),
        keymap.of([
          ...defaultKeymap,
          ...historyKeymap,
          indentWithTab,
          { key: "Mod-s", run: () => { void this.saveNow(); return true; } },
        ]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            if (this.open) this.open.dirty = true;
            this.updateStatus();
            this.schedulePreview();
            this.scheduleSave();
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
  // 锁生命周期（B1）
  // ============================================================

  /** 串行获取笔记锁：先释放旧锁再获取新锁。返回是否成功（false = 被其他窗口占用）。 */
  private async lockNote(rel: string): Promise<boolean> {
    const op = this.lockChain.then(async () => {
      if (this.lockedRel && this.lockedRel !== rel) {
        await releaseNoteLock(this.lockedRel).catch(() => {});
      }
      try {
        await acquireNoteLock(rel);
        this.lockedRel = rel;
        return true;
      } catch {
        return false;
      }
    });
    this.lockChain = op.then(() => {});
    return op;
  }

  /** 释放指定笔记的锁（幂等），并入锁链串行排队。 */
  private releaseLockFor(rel: string): void {
    this.lockChain = this.lockChain.then(() => releaseNoteLock(rel).catch(() => {}));
    if (this.lockedRel === rel) this.lockedRel = null;
  }

  /** 释放当前持有的锁（若持有）。编辑器销毁/占位时调用。 */
  private releaseOpenLock(): void {
    if (this.lockedRel) this.releaseLockFor(this.lockedRel);
  }

  // ============================================================
  // 图片导入
  // ============================================================

  private pickImage(): void {
    const n = this.open;
    if (!n || n.readOnly) return;
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.onchange = () => {
      const file = input.files?.[0];
      input.remove();
      if (file) void this.importPicked(file, n.rel);
    };
    input.click();
  }

  private async importPicked(file: File, noteRel: string): Promise<void> {
    if (file.size > MAX_IMAGE_BYTES) {
      this.setSave(`图片超过 20MB 上限：${file.name}`, "error");
      return;
    }
    try {
      const buf = await file.arrayBuffer();
      const res = await assetsImportBase64(noteRel, file.name, bytesToBase64(new Uint8Array(buf)));
      this.insertImageRef(relToNote(noteRel, res.rel), baseName(file.name));
      this.setSave(`已导入 ${file.name}`, "saved");
    } catch (err) {
      this.setSave(errText(err), "error");
    }
  }

  private async onDrop(paths: string[]): Promise<void> {
    const n = this.open;
    if (!n || n.readOnly) return;
    const images = paths.filter((p) => IMAGE_RE.test(p));
    if (images.length === 0) {
      this.setSave("仅支持图片文件（png/jpg/gif/webp/bmp/svg）", "warn");
      return;
    }
    let ok = 0;
    for (const src of images) {
      try {
        const res = await assetsImport(n.rel, src);
        this.insertImageRef(relToNote(n.rel, res.rel), baseName(src));
        ok++;
      } catch (err) {
        this.setSave(errText(err), "error");
      }
    }
    if (ok) this.setSave(`已导入 ${ok} 张图片`, "saved");
  }

  private insertImageRef(rel: string, name: string): void {
    if (!this.view) return;
    const sel = this.view.state.selection.main;
    const text = `![${name}](${rel})`;
    this.view.dispatch({
      changes: { from: sel.from, to: sel.to, insert: text },
      selection: EditorSelection.cursor(sel.from + text.length),
      userEvent: "input",
    });
    this.view.focus();
  }

  // ============================================================
  // 状态栏
  // ============================================================

  private updateStatus(): void {
    const wc = this.container?.querySelector<HTMLElement>("#ed-wc");
    const fmt = this.container?.querySelector<HTMLElement>("#ed-format");
    if (!wc || !fmt) return;
    const text = this.view?.state.doc.toString() ?? "";
    wc.textContent = `字数 ${text.replace(/\s/g, "").length}`;
    fmt.textContent = this.open
      ? this.open.rel.toLowerCase().endsWith(".md")
        ? "Markdown"
        : "纯文本"
      : "";
  }

  private setSave(text: string, cls: "" | "saved" | "warn" | "error"): void {
    const el = this.container?.querySelector<HTMLElement>("#ed-save");
    if (!el) return;
    el.textContent = text;
    el.className = "save-state" + (cls ? ` ${cls}` : "");
  }

  // ============================================================
  // 销毁
  // ============================================================

  /** 销毁实例：清定时器、退 doc-click 监听、释放 view/拖拽/锁、清空容器。幂等。 */
  destroy(): void {
    clearTimeout(this.saveTimer);
    clearTimeout(this.previewTimer);
    document.removeEventListener("click", this.onDocClickBound);
    this.destroyEditor();
    this.loadToken++; // 使进行中的 loadNote 失效，避免写回已销毁容器
    if (this.container) this.container.innerHTML = "";
    this.container = null;
  }
}
