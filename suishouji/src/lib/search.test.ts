// M2 搜索倒排索引单测（纯逻辑，无 DOM/Tauri 依赖）。
import { describe, expect, it } from "vitest";
import { NoteIndex, tokenize } from "./search";
import type { NoteMeta } from "../types";

/** 构造最小合法 NoteMeta；缺省字段用合理默认。 */
function note(partial: Partial<NoteMeta> & { path: string }): NoteMeta {
  return {
    id: partial.path,
    title: partial.path,
    format: "md",
    mtime: 0,
    imageCount: 0,
    tags: [],
    pinned: false,
    preview: "",
    searchText: "",
    ...partial,
  };
}

describe("tokenize", () => {
  it("CJK 逐字切分", () => {
    expect(tokenize("你好世界")).toEqual(["你", "好", "世", "界"]);
  });

  it("拉丁词转小写、保留数字", () => {
    expect(tokenize("Hello World")).toEqual(["hello", "world"]);
    expect(tokenize("note 2024")).toEqual(["note", "2024"]);
    expect(tokenize("abc123")).toEqual(["abc123"]);
  });

  it("中英混合与空白", () => {
    expect(tokenize("笔记 Note")).toEqual(["笔", "记", "note"]);
    expect(tokenize("a  b\nc")).toEqual(["a", "b", "c"]);
  });

  it("非字母数字分隔符断开拉丁词", () => {
    expect(tokenize("foo-bar_baz")).toEqual(["foo", "bar", "baz"]);
  });

  it("空串 / 纯符号返回空数组", () => {
    expect(tokenize("")).toEqual([]);
    expect(tokenize("---!!!")).toEqual([]);
  });
});

describe("NoteIndex", () => {
  it("build 后按 AND 命中，title 权重大于 body", () => {
    const index = new NoteIndex();
    index.build([
      note({ path: "a.md", title: "会议记录" }),
      note({ path: "b.md", title: "其他", searchText: "会议内容摘要" }),
    ]);
    // "会议" 同时命中 a（title）与 b（body），a 因 title 权重高排在前面
    expect(index.search("会议")).toEqual(["a.md", "b.md"]);
    // AND：两个词元须在同一篇命中 → 仅 b
    expect(index.search("会议 内容")).toEqual(["b.md"]);
  });

  it("未命中返回空数组", () => {
    const index = new NoteIndex();
    index.build([note({ path: "a.md", title: "会议记录" })]);
    expect(index.search("不存在")).toEqual([]);
  });

  it("空查询 / 纯符号查询返回空数组", () => {
    const index = new NoteIndex();
    index.build([note({ path: "a.md", title: "会议记录" })]);
    expect(index.search("")).toEqual([]);
    expect(index.search("   ")).toEqual([]);
  });

  it("tag 权重介于 title 与 body 之间", () => {
    const index = new NoteIndex();
    // a：tag 命中；b：body 命中；c：title 命中。权重 3 > 2 > 1
    index.build([
      note({ path: "a.md", title: "x", tags: ["会议"] }),
      note({ path: "b.md", title: "x", searchText: "会议" }),
      note({ path: "c.md", title: "会议" }),
    ]);
    expect(index.search("会议")).toEqual(["c.md", "a.md", "b.md"]);
  });

  it("重复 build 会清空旧索引（重建语义）", () => {
    const index = new NoteIndex();
    index.build([note({ path: "a.md", title: "会议记录" })]);
    index.build([note({ path: "b.md", title: "旅行计划" })]);
    expect(index.search("会议")).toEqual([]);
    expect(index.search("旅行")).toEqual(["b.md"]);
  });
});
