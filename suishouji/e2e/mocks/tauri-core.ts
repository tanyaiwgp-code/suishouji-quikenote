// E2E mock：内存笔记库，模拟 Tauri IPC（isTauri=false 的纯 Web 前端测试）。
// 命令名/参数/返回形状与 src/lib/api.ts 对齐（list/read/write/delete/settings/...）。
// 注：不注入 window.__TAURI_INTERNALS__，故 isTauri() 返回 false —— 窗口/拖拽/事件等
// 原生特性在 main.ts/quicknote.ts/editor.ts 里按 isTauri 分支自动跳过。
import type { AppSettings, NoteMeta } from "../../src/lib/types.gen";
import { emit } from "./tauri-event";

interface MockNote {
  rel: string;
  content: string;
  mtime: number;
  pinned: boolean;
  /** M9：set_note_title 覆盖的标题（优先于内容推导） */
  title?: string;
}

let counter = 1;
const byPath = new Map<string, MockNote>();
function seed(rel: string, content: string): void {
  byPath.set(rel, { rel, content, mtime: counter++ * 1000, pinned: false });
}
seed("收件箱/2026-08-15_090000.md", "# 会议记录\n\n明天 10 点评审，讨论 M7 发布。");
seed("项目/需求.txt", "纯文本草稿：随手记 E2E。");

// P0-数据安全：回收站内存 mock（trash_note → 移入 / restore_note → 回 byPath）
const trashById = new Map<string, { id: string; originalRel: string; title: string; format: "md" | "txt"; deletedAt: number; imageCount: number }>();

let settings: AppSettings = { root: "C:/mock-notes", autostart: false };

// 记录被调用的命令（供 E2E 断言数据链路），挂到 window 便于测试读取
const calls: string[] = [];
if (typeof window !== "undefined") {
  (window as unknown as { __MOCK_CALLS__?: string[] }).__MOCK_CALLS__ = calls;
}

function buildMeta(m: MockNote): NoteMeta {
  const format = m.rel.toLowerCase().endsWith(".md") ? "md" : "txt";
  const firstLine = m.content.split("\n")[0] ?? "";
  const title = m.title ?? (firstLine.replace(/^#\s*/, "").trim() || (m.rel.split("/").pop() ?? m.rel));
  return {
    id: `mock-${m.rel}`,
    title,
    path: m.rel,
    format,
    mtime: m.mtime,
    imageCount: 0,
    tags: [],
    pinned: m.pinned,
    preview: m.content.slice(0, 80),
    searchText: m.content,
  };
}

/** 与 Rust `list_notes` 一致：置顶 → mtime 倒序 → 字典序。 */
function sortedNotes(): NoteMeta[] {
  const all = [...byPath.values()].map(buildMeta);
  all.sort(
    (a, b) =>
      Number(b.pinned) - Number(a.pinned) || b.mtime - a.mtime || a.path.localeCompare(b.path),
  );
  return all;
}

export async function invoke<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  calls.push(cmd);
  switch (cmd) {
    case "list_notes":
      return sortedNotes() as T;
    case "read_note": {
      const rel = args.rel as string;
      const m = byPath.get(rel);
      if (!m) throw new Error(`not_found: ${rel}`);
      return m.content as T;
    }
    case "write_note": {
      const rel = args.rel as string;
      const content = args.content as string;
      const prev = byPath.get(rel);
      byPath.set(rel, {
        rel,
        content,
        mtime: prev ? prev.mtime + 1000 : counter++ * 1000,
        pinned: prev?.pinned ?? false,
      });
      return undefined as T;
    }
    case "delete_note":
      byPath.delete(args.rel as string);
      return undefined as T;
    // P0-数据安全：回收站命令 mock（与真实后端语义一致）
    case "trash_note": {
      const rel = args.rel as string;
      const m = byPath.get(rel);
      if (!m) throw new Error(`not_found: ${rel}`);
      byPath.delete(rel);
      trashById.set(`trash-${rel}`, {
        id: `trash-${rel}`,
        originalRel: m.rel,
        title: m.title ?? m.rel,
        format: m.rel.toLowerCase().endsWith(".md") ? "md" : "txt",
        deletedAt: counter++ * 1000,
        imageCount: m.imageCount,
      });
      emit("notes://changed");
      return trashById.get(`trash-${rel}`) as T;
    }
    case "list_trash":
      return [...trashById.values()].sort((a, b) => b.deletedAt - a.deletedAt) as T;
    case "restore_note": {
      const id = args.id as string;
      const t = trashById.get(id);
      if (!t) throw new Error(`not_found: ${id}`);
      trashById.delete(id);
      seed(t.originalRel, "");
      emit("notes://changed");
      return undefined as T;
    }
    case "purge_note":
      trashById.delete(args.id as string);
      return undefined as T;
    case "empty_trash": {
      const n = trashById.size;
      trashById.clear();
      return n as T;
    }
    case "backup_all":
    case "restore_backup":
    case "open_log_dir":
    case "get_crash_report":
      return undefined as T;
    case "set_note_title": {
      const rel = args.rel as string;
      const m = byPath.get(rel);
      if (!m) throw new Error(`not_found: ${rel}`);
      m.title = args.title as string;
      m.mtime = counter++ * 1000; // 更新 mtime → 列表重排
      emit("notes://changed"); // 触发前端 onNotesChanged → loadNotes 刷新（与真实后端一致）
      return undefined as T;
    }
    case "acquire_note_lock":
    case "release_note_lock":
      return undefined as T;
    case "get_app_settings":
      return { ...settings } as T;
    case "set_app_root":
      settings = { ...settings, root: args.root as string };
      return undefined as T;
    case "set_autostart":
      settings = { ...settings, autostart: args.enabled as boolean };
      return undefined as T;
    case "note_abs_path":
      return `C:/mock-notes/${args.rel}` as T;
    case "assets_import":
    case "assets_import_base64":
      // E2E 不测图片链路，占位空对象即可
      return {} as T;
    case "docx_import":
      return args.targetRel as T;
    case "docx_export":
    case "open_main_window":
    case "hide_quicknote":
      return undefined as T;
    default:
      throw new Error(`e2e mock: 未实现的命令 ${cmd}`);
  }
}

// --- P0-商用化：Tauri 插件（updater/log 等）静态导入 core 的 Channel/Resource ---
// E2E 是纯 Web 环境（isTauri=false），插件函数不会被调用，只需同名类让 ESM 具名导入不报错。
// 语义对齐 @tauri-apps/api/core，但去掉对 __TAURI_INTERNALS__ 的依赖。
export class Channel<T = unknown> {
  readonly id = 0;
  private handler: (message: T) => void = () => {};
  set onmessage(h: ((message: T) => void) | undefined) {
    this.handler = h ?? (() => {});
  }
  send(message: T): void {
    this.handler(message);
  }
}

export class Resource {
  readonly rid: number;
  constructor(rid: number) {
    this.rid = rid;
  }
  async close(): Promise<void> {
    /* E2E 下不会调用（isTauri=false），保持无操作 */
  }
}

export const convertFileSrc = (s: string): string => s;
export const isTauri = (): boolean => false;
