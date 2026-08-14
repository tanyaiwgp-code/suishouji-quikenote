// 轻量状态（nanostores）单测：默认值契约 + set/get 往返 + store↔search 集成编排。
import { beforeEach, describe, expect, it } from "vitest";
import { index, mobileView, navView, notes, query, selectedPath } from "./store";
import type { NoteMeta } from "../types";

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

beforeEach(() => {
  // 模块级单例：每个用例前重置，避免用例间互相污染
  notes.set([]);
  selectedPath.set(null);
  query.set("");
  navView.set("all");
  mobileView.set("list");
  index.build([]);
});

describe("atom 默认值契约", () => {
  it("初始状态符合 M2 布局默认", () => {
    expect(notes.get()).toEqual([]);
    expect(selectedPath.get()).toBeNull();
    expect(query.get()).toBe("");
    expect(navView.get()).toBe("all");
    expect(mobileView.get()).toBe("list");
  });
});

describe("set/get 往返", () => {
  it("selectedPath / query / navView / mobileView 可写可读", () => {
    selectedPath.set("收件箱/a.md");
    expect(selectedPath.get()).toBe("收件箱/a.md");

    query.set("会议");
    expect(query.get()).toBe("会议");

    navView.set("pinned");
    expect(navView.get()).toBe("pinned");

    mobileView.set("editor");
    expect(mobileView.get()).toBe("editor");
  });
});

describe("store ↔ search 编排契约", () => {
  it("notes 变更后重建 index，搜索即可命中（main.ts 的加载/监听流程）", () => {
    notes.set([note({ path: "a.md", title: "会议记录" })]);
    index.build(notes.get());
    expect(index.search("会议")).toEqual(["a.md"]);

    // 列表刷新（文件监听）：重建后旧词元失效
    notes.set([note({ path: "b.md", title: "旅行计划" })]);
    index.build(notes.get());
    expect(index.search("会议")).toEqual([]);
    expect(index.search("旅行")).toEqual(["b.md"]);
  });
});
