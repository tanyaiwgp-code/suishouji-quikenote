// 随手记 (suishouji) — Copyright (C) 2026 Tanya Wang
// SPDX-License-Identifier: AGPL-3.0-only
// 本软件按 GNU AGPL-3.0 发布；商业使用需另行授权（见 COMMERCIAL-LICENSE.md）。
//
// ============================================================
// 随手记 · 应用入口
// M0: 骨架搭建 — 主题切换 + 基础生命周期
// M2: 主窗口三栏 UI — 加载列表、搜索、导航、新建、文件监听刷新
// ============================================================

import "./styles.css";
import {
  ApiError,
  backupAll,
  emptyTrash,
  getAppSettings,
  listNotes,
  listTrash,
  onNotesChanged,
  openLogDir,
  purgeNote,
  restoreBackup,
  restoreNote,
  setAppRoot,
  setAutostart,
  trashNote,
  writeNote,
} from "./lib/api";
import { createEditor, type EditorInstance } from "./lib/editor";
import { applyDelete, index, mobileView, navView, notes, query, selectedPath, trash, type NavView } from "./lib/store";
import { buildNoteRel, inboxTimestampBase, type NoteExt } from "./lib/note-name";
import { applyFont, applyTheme, initTheme } from "./lib/theme";
import {
  applyViewMode,
  renderAll,
  renderSkeleton,
  renderShell,
  setEditor,
  updateListTitle,
} from "./ui";
import { checkForUpdates, notifyCrash } from "./lib/updater";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { getVersion } from "@tauri-apps/api/app";
import type { NoteMeta } from "./types";

/** 与 Rust `list_notes` 一致的排序：置顶 → mtime 倒序 → 路径字典序（B7 本地重排用）。 */
function byMtime(a: NoteMeta, b: NoteMeta): number {
  return Number(b.pinned) - Number(a.pinned) || b.mtime - a.mtime || a.path.localeCompare(b.path);
}

// --- 数据加载 ---
async function loadNotes(): Promise<void> {
  // M6：列表为空（首次加载）时先渲染骨架屏，避免白屏；刷新时保留旧列表不闪
  if (notes.get().length === 0) renderSkeleton();
  try {    const data = await listNotes();
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

// --- P0-数据安全：回收站数据加载 ---
async function loadTrash(): Promise<void> {
  try {
    trash.set(await listTrash());
  } catch (e) {
    console.error("加载回收站失败:", e);
    trash.set([]);
  }
  renderAll();
  applyViewMode();
}

// --- 新建笔记（收件箱，时间戳命名） ---
// B5：秒级时间戳同秒连续点击会碰撞同名文件 → 会话内序号 + 与现有列表查重兜底
let lastCreateBase = "";
let createSeq = 0;

async function createNote(ext: NoteExt = "md"): Promise<void> {
  const base = inboxTimestampBase();
  if (base === lastCreateBase) {
    createSeq += 1;
  } else {
    lastCreateBase = base;
    createSeq = 0;
  }
  // 兜底：与磁盘上已存在的笔记路径查重（跨会话同名文件）
  const { rel, seq } = buildNoteRel(base, ext, createSeq, notes.get().map((n) => n.path));
  createSeq = seq;
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

  // 导航（全部 / 收藏 / 回收站）
  document.querySelectorAll(".nav-item[data-view]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const v = (btn as HTMLElement).dataset.view as NavView;
      navView.set(v);
      document
        .querySelectorAll(".nav-item[data-view]")
        .forEach((b) => b.classList.toggle("active", b === btn));
      updateListTitle();
      // P0-数据安全：进入回收站视图时刷新条目
      if (v === "trash") {
        selectedPath.set(null);
        void loadTrash();
      } else {
        renderAll();
        applyViewMode();
      }
    });
  });

  // 新建：＋按钮弹出「新建 Markdown / 新建纯文本」菜单（M5），M6 补键盘导航与 aria
  const newNoteBtn = document.getElementById("new-note");
  const newMenu = document.getElementById("new-menu") as HTMLElement | null;
  const closeNewMenu = (): void => {
    if (newMenu) newMenu.hidden = true;
    newNoteBtn?.setAttribute("aria-expanded", "false");
  };
  const toggleNewMenu = (): void => {
    if (!newMenu) return;
    const willOpen = newMenu.hidden;
    newMenu.hidden = !willOpen;
    newNoteBtn?.setAttribute("aria-expanded", String(willOpen));
    if (willOpen) newMenu.querySelector<HTMLElement>("button")?.focus();
  };
  newNoteBtn?.addEventListener("click", (e) => {
    e.stopPropagation();
    toggleNewMenu();
  });
  newMenu?.querySelectorAll("button[data-ext]").forEach((btn) => {
    btn.addEventListener("click", () => {
      closeNewMenu();
      void createNote((btn as HTMLElement).dataset.ext as NoteExt);
    });
  });
  newMenu?.addEventListener("keydown", (e) => {
    const items = Array.from(newMenu.querySelectorAll<HTMLElement>("button"));
    const idx = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const dir = e.key === "ArrowDown" ? 1 : -1;
      items[(idx + dir + items.length) % items.length]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeNewMenu();
      newNoteBtn?.focus();
    }
  });
  document.addEventListener("click", (e) => {
    if (newMenu && !(e.target as HTMLElement).closest(".new-menu-wrap")) {
      closeNewMenu();
    }
  });

  // 笔记列表点击（事件委托）
  document.getElementById("note-list")?.addEventListener("click", (e) => {
    const card = (e.target as HTMLElement).closest<HTMLElement>(".note-card");
    if (!card) return;
    // P0-数据安全：回收站视图条目不进入编辑器（恢复/删除按钮另行处理）
    if (navView.get() === "trash") return;
    selectedPath.set(card.dataset.path ?? null);
    if (window.matchMedia("(max-width: 719px)").matches) {
      mobileView.set("editor");
    }
    applyViewMode();
    renderAll();
  });

  // M6：笔记卡片键盘可达（Enter/Space 选中，tabindex 由 ui.ts cardHtml 提供）
  document.getElementById("note-list")?.addEventListener("keydown", (e) => {
    // P0-数据安全：回收站视图键盘交互走回收站专属监听（恢复/删除）
    if (navView.get() === "trash") return;
    const card = (e.target as HTMLElement).closest<HTMLElement>(".note-card");
    if (!card) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      selectedPath.set(card.dataset.path ?? null);
      if (window.matchMedia("(max-width: 719px)").matches) mobileView.set("editor");
      applyViewMode();
      renderAll();
      return;
    }
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

  // --- M6：设置弹层 ---
  const backdrop = document.getElementById("settings-backdrop") as HTMLElement | null;
  const themeSel = document.getElementById("set-theme") as HTMLSelectElement | null;
  const fontSel = document.getElementById("set-font") as HTMLSelectElement | null;
  const openSettings = (): void => {
    if (!backdrop) return;
    // 打开时同步顶栏快速切换后的主题/字号
    if (themeSel) {
      const cur = localStorage.getItem("theme");
      themeSel.value = cur === "dark" ? "dark" : cur === "light" ? "light" : "system";
    }
    if (fontSel) fontSel.value = (localStorage.getItem("fontScale") as string) || "standard";
    backdrop.hidden = false;
    void loadSettingsForm();
  };
  document.getElementById("settings-btn")?.addEventListener("click", openSettings);
  document.getElementById("settings-close")?.addEventListener("click", () => {
    if (backdrop) backdrop.hidden = true;
  });
  backdrop?.addEventListener("click", (e) => {
    if (e.target === backdrop) backdrop.hidden = true;
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && backdrop && !backdrop.hidden) backdrop.hidden = true;
  });

  // 主题三态
  themeSel?.addEventListener("change", () => {
    localStorage.setItem("theme", themeSel.value);
    applyTheme(themeSel.value);
  });
  // 字号三档
  fontSel?.addEventListener("change", () => {
    localStorage.setItem("fontScale", fontSel.value);
    applyFont(fontSel.value);
  });
  // 开机自启
  const autoBox = document.getElementById("set-autostart") as HTMLInputElement | null;
  autoBox?.addEventListener("change", () => {
    void setAutostart(autoBox.checked).catch((e) => {
      console.error("设置开机自启失败:", e);
      autoBox.checked = !autoBox.checked;
    });
  });
  // 数据目录（重启生效）
  document.getElementById("set-root-pick")?.addEventListener("click", async () => {
    const picked = await open({ title: "选择数据目录", directory: true, multiple: false }).catch(
      () => null,
    );
    if (!picked || typeof picked !== "string") return;
    try {
      await setAppRoot(picked);
      const rootEl = document.getElementById("set-root");
      if (rootEl) rootEl.textContent = picked;
      checkOneDrive(picked);
      window.alert("数据目录已更新，重启应用后生效");
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : String(e);
      console.error("设置数据目录失败:", e);
      window.alert(`设置数据目录失败：${msg}`);
    }
  });

  // --- P0-数据安全：一键备份 / 恢复 ---
  document.getElementById("set-backup")?.addEventListener("click", async () => {
    const target = await save({
      title: "备份全部笔记",
      defaultPath: `随手记备份_${backupStamp()}.zip`,
      filters: [{ name: "备份文件", extensions: ["zip"] }],
    });
    if (!target || typeof target !== "string") return;
    try {
      const n = await backupAll(target);
      window.alert(`备份完成：已打包 ${n} 个文件到\n${target}`);
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : String(e);
      console.error("备份失败:", e);
      window.alert(`备份失败：${msg}`);
    }
  });

  document.getElementById("set-restore")?.addEventListener("click", async () => {
    const picked = await open({
      title: "从备份恢复",
      filters: [{ name: "备份文件", extensions: ["zip"] }],
      multiple: false,
    });
    if (!picked || typeof picked !== "string") return;
    const ok = window.confirm(
      "从备份恢复会用备份内容覆盖当前同名笔记，且不可撤销。\n建议先「备份全部」一份当前数据。\n\n确定继续？",
    );
    if (!ok) return;
    try {
      const n = await restoreBackup(picked);
      await loadNotes();
      window.alert(`恢复完成：已还原 ${n} 个文件。`);
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : String(e);
      console.error("恢复失败:", e);
      window.alert(`恢复失败：${msg}`);
    }
  });

  // --- P0-商用化：关于区（日志目录 / 检查更新） ---
  document.getElementById("set-log-dir")?.addEventListener("click", () => {
    void openLogDir().catch((e) => {
      const msg = e instanceof ApiError ? e.message : String(e);
      console.error("打开日志目录失败:", e);
      window.alert(`打开日志目录失败：${msg}`);
    });
  });
  document.getElementById("set-check-update")?.addEventListener("click", () => {
    void checkForUpdates(true).catch(() => {});
  });

  // --- M7 删除（P0-数据安全改为软删除：确认 → 释放锁 → 移入回收站 → 本地刷新） ---
  const confirmBackdrop = document.getElementById("confirm-backdrop") as HTMLElement | null;
  const confirmText = document.getElementById("confirm-text") as HTMLElement | null;
  const ctxMenu = document.getElementById("ctx-menu") as HTMLElement | null;
  let pendingDeleteRel: string | null = null;
  let ctxRel: string | null = null;

  const closeConfirm = (): void => {
    if (confirmBackdrop) confirmBackdrop.hidden = true;
    pendingDeleteRel = null;
  };
  const openConfirm = (rel: string): void => {
    pendingDeleteRel = rel;
    const n = notes.get().find((x) => x.path === rel);
    if (confirmText) {
      confirmText.textContent = n
        ? `确定删除「${n.title}」？将移入回收站，可随时恢复。`
        : "确定删除该笔记？将移入回收站，可随时恢复。";
    }
    if (confirmBackdrop) confirmBackdrop.hidden = false;
  };
  const doDelete = async (): Promise<void> => {
    if (!pendingDeleteRel) return;
    const rel = pendingDeleteRel;
    closeConfirm();
    editor.render(null); // 先同步释放文件锁并阻断保存回写（saveTimer 因 open=null 提前返回）
    try {
      await trashNote(rel); // P0-数据安全：软删除（移入回收站，不物理删除）
      applyDelete(rel);
      renderAll();
      applyViewMode();
    } catch (e) {
      console.error("删除笔记失败:", e);
      await loadNotes(); // 文件仍在磁盘，重载恢复列表
    }
  };

  document.getElementById("confirm-delete")?.addEventListener("click", () => {
    void doDelete();
  });
  document.getElementById("confirm-cancel")?.addEventListener("click", closeConfirm);
  confirmBackdrop?.addEventListener("click", (e) => {
    if (e.target === confirmBackdrop) closeConfirm();
  });

  // 工具栏删除按钮 → 打开确认框（editor.ts 通过 note:delete-request 派发）
  window.addEventListener("note:delete-request", (e) => {
    const rel = (e as CustomEvent<{ rel: string }>).detail?.rel;
    if (rel) openConfirm(rel);
  });

  // 列表右键菜单（fixed 定位，防视口边缘溢出）
  const closeCtxMenu = (): void => {
    if (ctxMenu) ctxMenu.hidden = true;
    ctxRel = null;
  };
  const showCtxMenu = (x: number, y: number): void => {
    if (!ctxMenu) return;
    const w = ctxMenu.offsetWidth || 160;
    const h = ctxMenu.offsetHeight || 40;
    ctxMenu.style.left = `${Math.min(x, window.innerWidth - w - 4)}px`;
    ctxMenu.style.top = `${Math.min(y, window.innerHeight - h - 4)}px`;
    ctxMenu.hidden = false;
  };
  document.getElementById("note-list")?.addEventListener("contextmenu", (e) => {
    const card = (e.target as HTMLElement).closest<HTMLElement>(".note-card");
    if (!card) return;
    e.preventDefault();
    ctxRel = card.dataset.path ?? null;
    if (ctxRel) showCtxMenu(e.clientX, e.clientY);
  });
  document.getElementById("ctx-delete")?.addEventListener("click", () => {
    const rel = ctxRel; // 先取再关菜单（closeCtxMenu 会清 ctxRel）
    closeCtxMenu();
    if (rel) openConfirm(rel);
  });
  document.addEventListener("click", (e) => {
    if (ctxMenu && !(e.target as HTMLElement).closest(".ctx-menu")) closeCtxMenu();
  });

  // Esc：右键菜单 / 删除确认 关闭（设置弹层 Esc 见上方独立监听）
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    if (ctxMenu && !ctxMenu.hidden) closeCtxMenu();
    if (confirmBackdrop && !confirmBackdrop.hidden) closeConfirm();
  });

  // M7 修正：键盘 Del 用全局监听（删当前选中笔记）。
  // 原因：点卡片选中会触发 renderList() innerHTML 全量重绘，销毁刚聚焦的卡片 DOM，焦点丢失到 body，
  // 列表内 keydown 委托因此收不到 Delete；全局监听不依赖焦点位置，排除编辑器/输入框即可。
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Delete") return;
    const t = e.target as HTMLElement;
    // 编辑器/输入框内 Delete 是编辑操作，绝不触发删除
    if (t.closest(".cm-editor, input, textarea, select, [contenteditable='true']")) return;
    const rel = selectedPath.get();
    if (rel) {
      e.preventDefault();
      openConfirm(rel);
    }
  });

  // --- P0-数据安全：回收站交互（恢复 / 永久删除 / 清空） ---
  const noteListEl = document.getElementById("note-list");

  const doRestore = async (id: string): Promise<void> => {
    try {
      await restoreNote(id);
      await loadTrash(); // 恢复后刷新回收站 + 列表
      await loadNotes();
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : String(e);
      console.error("恢复笔记失败:", e);
      window.alert(`恢复失败：${msg}`);
    }
  };

  const doPurge = async (id: string, title: string): Promise<void> => {
    if (!window.confirm(`永久删除「${title}」？此操作不可恢复。`)) return;
    try {
      await purgeNote(id);
      await loadTrash();
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : String(e);
      console.error("永久删除失败:", e);
      window.alert(`删除失败：${msg}`);
    }
  };

  const doEmptyTrash = async (): Promise<void> => {
    if (!window.confirm("清空回收站？所有条目将被永久删除，不可恢复。")) return;
    try {
      const n = await emptyTrash();
      await loadTrash();
      window.alert(`已清空回收站（${n} 条）。`);
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : String(e);
      console.error("清空回收站失败:", e);
      window.alert(`清空失败：${msg}`);
    }
  };

  document.getElementById("note-list")?.addEventListener("click", (e) => {
    const target = e.target as HTMLElement;
    if (target.closest("#trash-empty")) {
      void doEmptyTrash();
      return;
    }
    const restoreBtn = target.closest<HTMLElement>(".restore-btn");
    if (restoreBtn?.dataset.trashId) {
      void doRestore(restoreBtn.dataset.trashId);
      return;
    }
    const purgeBtn = target.closest<HTMLElement>(".purge-btn");
    if (purgeBtn?.dataset.trashId) {
      const card = purgeBtn.closest<HTMLElement>(".trash-card");
      void doPurge(purgeBtn.dataset.trashId, card?.dataset.title ?? "该笔记");
      return;
    }
    // 回收站视图点击条目本身不进入编辑器
    if (navView.get() === "trash" && target.closest(".trash-card")) return;
  });

  // 回收站键盘可达（Enter = 恢复）
  noteListEl?.addEventListener("keydown", (e) => {
    if (navView.get() !== "trash" || e.key !== "Enter") return;
    const btn = (e.target as HTMLElement).closest<HTMLElement>(".restore-btn, .purge-btn");
    if (!btn?.dataset.trashId) return;
    e.preventDefault();
    if (btn.classList.contains("restore-btn")) void doRestore(btn.dataset.trashId);
    else {
      const card = btn.closest<HTMLElement>(".trash-card");
      void doPurge(btn.dataset.trashId, card?.dataset.title ?? "该笔记");
    }
  });
}

/** M6：加载设置表单（数据根目录 + 开机自启状态）。 */
async function loadSettingsForm(): Promise<void> {
  try {
    const s = await getAppSettings();
    const rootEl = document.getElementById("set-root");
    if (rootEl) rootEl.textContent = s.root;
    const autoBox = document.getElementById("set-autostart") as HTMLInputElement | null;
    if (autoBox) autoBox.checked = s.autostart;
    checkOneDrive(s.root);
  } catch (e) {
    console.error("加载设置失败:", e);
  }
  // P0-商用化：显示版本号
  try {
    const versionEl = document.getElementById("set-version");
    if (versionEl && isTauri()) {
      const v = await getVersion();
      versionEl.textContent = `随手记 v${v}`;
    }
  } catch {
    /* 非 Tauri 环境忽略 */
  }
}

/** P0-数据安全：OneDrive 重定向提示（数据目录在 OneDrive 路径下时显示警告行）。 */
function checkOneDrive(root: string): void {
  const warn = document.getElementById("onedrive-warn");
  if (!warn) return;
  const isOneDrive = /[\\/]OneDrive[\\/]/i.test(root);
  warn.hidden = !isOneDrive;
}

/** 备份文件名时间戳：YYYYMMDD-HHmm。 */
function backupStamp(): string {
  const d = new Date();
  const pad = (x: number) => String(x).padStart(2, "0");
  return `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}`;
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
  // P0-商用化：启动后延迟检查更新 + 崩溃提示，不抢首屏、不阻塞核心功能
  window.setTimeout(() => {
    void checkForUpdates();
    void notifyCrash();
  }, 3000);
});
