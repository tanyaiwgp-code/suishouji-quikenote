// IPC 封装单测：mock @tauri-apps/api/core，验证各函数调用的命令名 + 参数形状，及 ApiError 归一化。
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  acquireNoteLock,
  ApiError,
  assetsImport,
  assetsImportBase64,
  deleteNote,
  hideQuicknote,
  listNotes,
  noteAbsPath,
  openMainWindow,
  readNote,
  releaseNoteLock,
  writeNote,
} from "./api";

// 仅 mock core 模块：invoke/convertFileSrc 均为 vi.fn；event 模块（listen）未被调用故不需 mock。
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn(),
}));

const mockedInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockedInvoke.mockReset();
});

describe("命令名与参数形状", () => {
  it("listNotes 调用 list_notes，无参数", async () => {
    mockedInvoke.mockResolvedValue([]);
    await listNotes();
    expect(mockedInvoke).toHaveBeenCalledWith("list_notes", undefined);
  });

  it("readNote/writeNote/deleteNote 参数形状（rel/content）", async () => {
    mockedInvoke.mockResolvedValue(undefined);
    await readNote("收件箱/a.md");
    expect(mockedInvoke).toHaveBeenCalledWith("read_note", { rel: "收件箱/a.md" });

    await writeNote("收件箱/a.md", "# 正文");
    expect(mockedInvoke).toHaveBeenCalledWith("write_note", {
      rel: "收件箱/a.md",
      content: "# 正文",
    });

    await deleteNote("收件箱/a.md");
    expect(mockedInvoke).toHaveBeenCalledWith("delete_note", { rel: "收件箱/a.md" });
  });

  it("锁命令参数形状", async () => {
    mockedInvoke.mockResolvedValue(undefined);
    await acquireNoteLock("收件箱/a.md");
    expect(mockedInvoke).toHaveBeenCalledWith("acquire_note_lock", { rel: "收件箱/a.md" });
    await releaseNoteLock("收件箱/a.md");
    expect(mockedInvoke).toHaveBeenCalledWith("release_note_lock", { rel: "收件箱/a.md" });
  });

  it("图片导入命令参数形状（noteRel/sourcePath、noteRel/filename/data）", async () => {
    mockedInvoke.mockResolvedValue({ rel: "a/assets/1.png", count: 1 });
    await assetsImport("收件箱/a.md", "C:\\x\\1.png");
    expect(mockedInvoke).toHaveBeenCalledWith("assets_import", {
      noteRel: "收件箱/a.md",
      sourcePath: "C:\\x\\1.png",
    });

    await assetsImportBase64("收件箱/a.md", "1.png", "aGVsbG8=");
    expect(mockedInvoke).toHaveBeenCalledWith("assets_import_base64", {
      noteRel: "收件箱/a.md",
      filename: "1.png",
      data: "aGVsbG8=",
    });
  });

  it("noteAbsPath / 窗口控制命令", async () => {
    mockedInvoke.mockResolvedValue("C:\\root\\a.md");
    await noteAbsPath("收件箱/a.md");
    expect(mockedInvoke).toHaveBeenCalledWith("note_abs_path", { rel: "收件箱/a.md" });

    mockedInvoke.mockResolvedValue(undefined);
    await openMainWindow();
    expect(mockedInvoke).toHaveBeenCalledWith("open_main_window", undefined);
    await hideQuicknote();
    expect(mockedInvoke).toHaveBeenCalledWith("hide_quicknote", undefined);
  });
});

describe("invoke 错误传播", () => {
  it("Rust 端结构化对象 rejection 转为 ApiError（带 code）", async () => {
    mockedInvoke.mockRejectedValue({ code: "locked", message: "笔记被锁定：a.md" });
    await expect(readNote("a.md")).rejects.toMatchObject({
      name: "ApiError",
      code: "locked",
      message: "笔记被锁定：a.md",
    });
  });
});

describe("ApiError.from 多形态解析", () => {
  it("形态 1：直接对象 {code, message}", () => {
    const e = ApiError.from({ code: "not_found", message: "资源不存在：a" });
    expect(e).toBeInstanceOf(ApiError);
    expect(e.code).toBe("not_found");
    expect(e.message).toBe("资源不存在：a");
  });

  it("形态 2：Error.message 为 JSON 字符串", () => {
    const e = ApiError.from(new Error('{"code":"image_too_large","message":"图片超过 20MB 上限：x"}'));
    expect(e.code).toBe("image_too_large");
    expect(e.message).toBe("图片超过 20MB 上限：x");
  });

  it("形态 3：纯字符串/非 JSON 降级为 code=unknown 并去 Error: 前缀", () => {
    const e = ApiError.from(new Error("笔记被锁定：a"));
    expect(e.code).toBe("unknown");
    expect(e.message).toBe("笔记被锁定：a");

    const e2 = ApiError.from("Error: 某错误");
    expect(e2.code).toBe("unknown");
    expect(e2.message).toBe("某错误");

    // 恰为合法 JSON 但缺 code/message 的字符串 → 形态 3（不误解析）
    const e3 = ApiError.from(new Error('"abc"'));
    expect(e3.code).toBe("unknown");
    expect(e3.message).toBe('"abc"');
  });

  it("null / undefined 不崩溃", () => {
    expect(ApiError.from(null).code).toBe("unknown");
    expect(ApiError.from(undefined).code).toBe("unknown");
  });
});
