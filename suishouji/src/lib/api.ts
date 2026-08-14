// 类型化 IPC 封装（薄封装，命令名与 Rust commands.rs / build.rs 白名单 / capabilities 一致，
// 由 contract.test.ts 契约测试锁定）。
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { NoteMeta, AssetImport, AppSettings } from "../types";

// --- 结构化错误 ---
// Rust 端 CommandError 序列化为 { code, message }（见 src-tauri/src/core/error.rs）。
// Tauri 前端 reject 的实际形态可能是对象本身，也可能是 Error.message 里的 JSON 字符串，
// 故 from() 做多形态解析；无法解析时 fallback code="unknown"。
export class ApiError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }

  static from(err: unknown): ApiError {
    // 形态 1：Rust 端结构化对象 { code, message }
    if (typeof err === "object" && err !== null) {
      const obj = err as { code?: unknown; message?: unknown };
      if (typeof obj.code === "string" && typeof obj.message === "string") {
        return new ApiError(obj.code, obj.message);
      }
    }
    // 形态 2：Error.message 为 JSON 字符串 {"code":...,"message":...}
    const raw = err instanceof Error ? err.message : String(err);
    try {
      const parsed = JSON.parse(raw) as { code?: unknown; message?: unknown };
      if (typeof parsed.code === "string" && typeof parsed.message === "string") {
        return new ApiError(parsed.code, parsed.message);
      }
    } catch {
      // 非 JSON，落入形态 3
    }
    // 形态 3：纯字符串（无结构），去掉 "Error: " 前缀
    return new ApiError("unknown", raw.replace(/^Error:\s*/, ""));
  }
}

/** invoke 包装：rejection 统一转 ApiError，供上层按 code 分支。 */
async function invokeOrThrow<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    throw ApiError.from(err);
  }
}

export const listNotes = (): Promise<NoteMeta[]> => invokeOrThrow<NoteMeta[]>("list_notes");
export const readNote = (rel: string): Promise<string> => invokeOrThrow<string>("read_note", { rel });
export const writeNote = (rel: string, content: string): Promise<void> =>
  invokeOrThrow<void>("write_note", { rel, content });
export const deleteNote = (rel: string): Promise<void> => invokeOrThrow<void>("delete_note", { rel });
/** M9：设置笔记 frontmatter 标题（后端改文件并广播刷新）。 */
export const setNoteTitle = (rel: string, title: string): Promise<void> =>
  invokeOrThrow<void>("set_note_title", { rel, title });
export const acquireNoteLock = (rel: string): Promise<void> =>
  invokeOrThrow<void>("acquire_note_lock", { rel });
export const releaseNoteLock = (rel: string): Promise<void> =>
  invokeOrThrow<void>("release_note_lock", { rel });

// --- M3：图片导入 & 绝对路径 ---

/** 拖入图片：Rust 复制到 `笔记名/assets/`，返回相对引用。 */
export const assetsImport = (noteRel: string, sourcePath: string): Promise<AssetImport> =>
  invokeOrThrow<AssetImport>("assets_import", { noteRel, sourcePath });

/** 工具栏选图：base64 字节写入 `笔记名/assets/`，返回相对引用。 */
export const assetsImportBase64 = (
  noteRel: string,
  filename: string,
  data: string,
): Promise<AssetImport> => invokeOrThrow<AssetImport>("assets_import_base64", { noteRel, filename, data });

/** 返回笔记绝对路径（供 convertFileSrc 渲染 assets/ 图片）。 */
export const noteAbsPath = (rel: string): Promise<string> => invokeOrThrow<string>("note_abs_path", { rel });

// --- M5：DOCX 导入导出 ---

/** 导入 DOCX（源文件路径 + 目标 md 相对路径）→ 转 MD 存库，返回新笔记 rel。 */
export const docxImport = (sourcePath: string, targetRel: string): Promise<string> =>
  invokeOrThrow<string>("docx_import", { sourcePath, targetRel });

/** 导出当前 MD 笔记为 DOCX（写入用户选择的保存路径）。 */
export const docxExport = (rel: string, targetPath: string): Promise<void> =>
  invokeOrThrow<void>("docx_export", { rel, targetPath });

// --- M6：应用设置 ---

/** 读取应用设置（数据根目录 + 开机自启状态）。 */
export const getAppSettings = (): Promise<AppSettings> =>
  invokeOrThrow<AppSettings>("get_app_settings");
/** 设置数据根目录（写 settings.json，重启后生效）。 */
export const setAppRoot = (root: string): Promise<void> => invokeOrThrow<void>("set_app_root", { root });
/** 开关开机自启。 */
export const setAutostart = (enabled: boolean): Promise<void> =>
  invokeOrThrow<void>("set_autostart", { enabled });

/** 导出 convertFileSrc，供预览图片路径重写。 */
export { convertFileSrc };

/** 订阅笔记目录外部改动事件（M2-4），返回取消订阅函数。 */
export function onNotesChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("notes://changed", () => cb());
}

// --- M4：窗口控制（Rust 端执行，避免给浮窗授予逐窗口 JS 权限） ---

/** 显示并聚焦主窗口（同时隐藏快速记录浮窗）。 */
export const openMainWindow = (): Promise<void> => invokeOrThrow<void>("open_main_window");
/** 隐藏快速记录浮窗。 */
export const hideQuicknote = (): Promise<void> => invokeOrThrow<void>("hide_quicknote");
