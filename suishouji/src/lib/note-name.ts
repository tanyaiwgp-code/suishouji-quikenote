// 收件箱时间戳笔记命名（M5 抽自 main.ts createNote / quicknote.ts draftNew，供两者复用并纯测）。
// 命名规则：`收件箱/YYYY-MM-DD_HHMMSS[_<seq>].<ext>`，同秒碰撞加序号 + 与现有列表查重兜底。

export type NoteExt = "md" | "txt";

/** 收件箱时间戳 base（不含扩展名）。 */
export function inboxTimestampBase(date = new Date()): string {
  const pad = (x: number) => String(x).padStart(2, "0");
  return (
    `收件箱/${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
    `_${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`
  );
}

/**
 * 由 base + 扩展名 + 会话序号 + 现有路径集合构造不冲突的相对路径。
 * 返回实际使用的 `seq`（可能因查重递增），调用方应回存以保持会话内递增。
 */
export function buildNoteRel(
  base: string,
  ext: NoteExt,
  seq: number,
  existing: readonly string[],
): { rel: string; seq: number } {
  let rel = seq === 0 ? `${base}.${ext}` : `${base}_${seq}.${ext}`;
  const used = new Set(existing);
  let s = seq;
  while (used.has(rel)) {
    s += 1;
    rel = `${base}_${s}.${ext}`;
  }
  return { rel, seq: s };
}
