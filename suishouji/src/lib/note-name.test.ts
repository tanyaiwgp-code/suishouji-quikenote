// 收件箱笔记命名（M5）单测。
import { describe, expect, it } from "vitest";
import { buildNoteRel, inboxTimestampBase } from "./note-name";

describe("inboxTimestampBase", () => {
  it("格式化为 收件箱/YYYY-MM-DD_HHMMSS", () => {
    expect(inboxTimestampBase(new Date(2026, 7, 14, 9, 8, 7))).toBe("收件箱/2026-08-14_090807");
  });
});

describe("buildNoteRel", () => {
  it("seq=0 不带序号；seq>0 带 _seq", () => {
    expect(buildNoteRel("收件箱/2026-08-14_090807", "md", 0, []).rel).toBe(
      "收件箱/2026-08-14_090807.md",
    );
    expect(buildNoteRel("收件箱/2026-08-14_090807", "txt", 2, []).rel).toBe(
      "收件箱/2026-08-14_090807_2.txt",
    );
  });

  it("与现有路径冲突时递增序号并回传实际 seq", () => {
    const existing = ["收件箱/2026-08-14_090807.md", "收件箱/2026-08-14_090807_1.md"];
    const r = buildNoteRel("收件箱/2026-08-14_090807", "md", 1, existing);
    expect(r.rel).toBe("收件箱/2026-08-14_090807_2.md");
    expect(r.seq).toBe(2);
  });

  it("txt 与 md 使用不同扩展名互不冲突", () => {
    expect(buildNoteRel("收件箱/x", "txt", 0, ["收件箱/x.md"]).rel).toBe("收件箱/x.txt");
  });
});
