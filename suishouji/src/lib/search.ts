// M2-3 搜索倒排索引：CJK 单字 + 拉丁单词切分，构建于内存，随笔记列表重建。
// 200 条笔记规模下查询 < 50ms（纯 Map 查找）。

import type { NoteMeta } from "../types";

/** CJK 单字 + 拉丁单词切分（转小写）。中文无乱码：按 Unicode 字符逐字。 */
export function tokenize(text: string): string[] {
  const lower = text.toLowerCase();
  const tokens: string[] = [];
  let word = "";
  for (const ch of lower) {
    if (isCjk(ch)) {
      if (word) {
        tokens.push(word);
        word = "";
      }
      tokens.push(ch);
    } else if (/[a-z0-9]/.test(ch)) {
      word += ch;
    } else {
      if (word) {
        tokens.push(word);
        word = "";
      }
    }
  }
  if (word) tokens.push(word);
  return tokens;
}

function isCjk(ch: string): boolean {
  const code = ch.codePointAt(0)!;
  return (
    (code >= 0x4e00 && code <= 0x9fff) || // CJK 统一表意文字
    (code >= 0x3400 && code <= 0x4dbf) || // CJK 扩展 A
    (code >= 0x3000 && code <= 0x30ff) || // CJK 标点 + 假名
    (code >= 0xac00 && code <= 0xd7af) // 谚文
  );
}

const WEIGHT_TITLE = 3;
const WEIGHT_TAG = 2;
const WEIGHT_BODY = 1;

/** 倒排索引：token → path → 权重和。 */
export class NoteIndex {
  private postings = new Map<string, Map<string, number>>();

  /** 全量重建。在笔记列表刷新（加载/文件监听触发）后调用。 */
  build(notes: NoteMeta[]): void {
    this.postings.clear();
    for (const n of notes) {
      this.add(n.path, n.title, WEIGHT_TITLE);
      this.add(n.path, n.tags.join(" "), WEIGHT_TAG);
      this.add(n.path, n.searchText, WEIGHT_BODY);
    }
  }

  /** 查询：全部词元须命中（AND），按权重和降序返回命中 path 列表。 */
  search(query: string): string[] {
    const tokens = [...new Set(tokenize(query))].filter((t) => t.length > 0);
    if (tokens.length === 0) return [];

    const first = this.postings.get(tokens[0]);
    if (!first) return [];

    const scored: Array<[string, number]> = [];
    for (const [path, base] of first) {
      let total = base;
      let allHit = true;
      for (let i = 1; i < tokens.length; i++) {
        const w = this.postings.get(tokens[i])?.get(path);
        if (w === undefined) {
          allHit = false;
          break;
        }
        total += w;
      }
      if (allHit) scored.push([path, total]);
    }
    scored.sort((a, b) => b[1] - a[1]);
    return scored.map(([path]) => path);
  }

  private add(path: string, text: string, weight: number): void {
    for (const t of tokenize(text)) {
      let m = this.postings.get(t);
      if (!m) {
        m = new Map();
        this.postings.set(t, m);
      }
      m.set(path, (m.get(path) ?? 0) + weight);
    }
  }
}
