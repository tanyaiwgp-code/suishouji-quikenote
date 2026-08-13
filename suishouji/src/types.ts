// 与 Rust core::model::NoteMeta 对应的 TS 镜像（camelCase 一致）

export type NoteFormat = "md" | "txt";

export interface NoteMeta {
  id: string;
  title: string;
  path: string;
  format: NoteFormat;
  mtime: number;
  imageCount: number;
  tags: string[];
  pinned: boolean;
  preview: string;
  /** 正文前 500 字，供搜索倒排索引（M2-3） */
  searchText: string;
}
