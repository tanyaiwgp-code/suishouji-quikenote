// 快速记录浮窗入口（M4）：全新草稿 → 自动保存 → finalize 落盘/删除 → 隐藏。
// 复用编辑器工厂的精简壳（隐藏返回键/模式按钮/状态栏/标题徽章，保留工具栏图片按钮）。
import "./styles.css";
import { deleteNote, hideQuicknote, listNotes, openMainWindow, writeNote } from "./lib/api";
import { buildNoteRel, inboxTimestampBase } from "./lib/note-name";
import { createEditor, type EditorInstance } from "./lib/editor";
import { initTheme } from "./lib/theme";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";

let editor: EditorInstance;
let currentRel = "";
let lastDraftBase = "";
let draftSeq = 0;

/** 新建一条收件箱草稿（时间戳命名，镜像 main.ts createNote 的查重逻辑，固定 .md）。 */
async function draftNew(): Promise<void> {
  const base = inboxTimestampBase();
  if (base === lastDraftBase) {
    draftSeq += 1;
  } else {
    lastDraftBase = base;
    draftSeq = 0;
  }
  try {
    const existing = (await listNotes()).map((n) => n.path);
    const { rel, seq } = buildNoteRel(base, "md", draftSeq, existing);
    draftSeq = seq;
    await writeNote(rel, "");
    const meta = (await listNotes()).find((n) => n.path === rel) ?? null;
    if (!meta) return;
    currentRel = rel;
    (document.getElementById("qn-title") as HTMLInputElement).value = "";
    editor.render(meta);
    editor.focus();
  } catch (e) {
    console.error("新建快速记录草稿失败:", e);
  }
}

/** 落盘并隐藏：空正文 → 删除草稿；有正文 → 写回（标题写入 frontmatter）。 */
async function finalize(openMain: boolean): Promise<void> {
  await editor.flushSave(); // 清掉防抖定时器，避免后续 autosave 覆盖 frontmatter 版本
  const body = editor.getContent();
  const title = (document.getElementById("qn-title") as HTMLInputElement).value.trim();
  if (!currentRel) return;
  if (!body.trim()) {
    await deleteNote(currentRel).catch(() => {});
  } else {
    // frontmatter::parse 取 `---` 后为 body，单换行避免重开前导空行
    const content = title ? `---\ntitle: ${title}\n---\n${body}` : body;
    await writeNote(currentRel, content).catch(() => {});
  }
  if (openMain) {
    await openMainWindow().catch(() => {});
  } else {
    await hideQuicknote().catch(() => {});
  }
}

/** Esc：有内容先确认，丢弃则删除草稿并隐藏。 */
async function discard(): Promise<void> {
  const body = editor.getContent();
  const title = (document.getElementById("qn-title") as HTMLInputElement).value.trim();
  if ((body.trim() || title) && !window.confirm("丢弃这条快速记录？")) return;
  if (currentRel) await deleteNote(currentRel).catch(() => {});
  await hideQuicknote().catch(() => {});
}

function wireEvents(): void {
  const titleInput = document.getElementById("qn-title") as HTMLInputElement;
  titleInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.ctrlKey) {
      e.preventDefault();
      void finalize(false); // 标题回车 = 保存并关闭
    }
  });
  document.getElementById("qn-save")?.addEventListener("click", () => void finalize(false));
  document.getElementById("qn-open")?.addEventListener("click", () => void finalize(true));

  document.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && e.ctrlKey) {
      e.preventDefault();
      void finalize(e.shiftKey); // Ctrl+Enter=存并关；Ctrl+Shift+Enter=存并开主窗口
    } else if (e.key === "Escape") {
      void discard();
    }
  });

  if (isTauri()) {
    void listen("quicknote:show", () => {
      void draftNew();
    }).catch(() => {});
    getCurrentWindow()
      .onCloseRequested(async (event) => {
        event.preventDefault();
        await finalize(false); // 关窗（Alt+F4）= 自动保存草稿
      })
      .catch(() => {});
  }
}

window.addEventListener("DOMContentLoaded", () => {
  initTheme();
  const app = document.getElementById("app") as HTMLElement;
  app.innerHTML = `
    <div class="quicknote">
      <header class="qn-header">
        <input id="qn-title" class="qn-title-input" type="text"
          placeholder="标题（可选，回车保存）" maxlength="120" autocomplete="off" />
      </header>
      <div class="qn-body" id="qn-body"></div>
      <footer class="qn-footer">
        <button id="qn-open" class="btn-secondary">打开主窗口</button>
        <button id="qn-save" class="btn-primary">保存并关闭</button>
      </footer>
    </div>`;
  editor = createEditor(document.getElementById("qn-body") as HTMLElement, {
    backButton: false,
    modeButtons: false,
    statusBar: false,
    titleBadge: false,
  });
  wireEvents();
});
