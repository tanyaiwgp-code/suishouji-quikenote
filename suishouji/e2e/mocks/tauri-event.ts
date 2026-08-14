// E2E mock：@tauri-apps/api/event 的 listen —— 让后端事件（如 notes://changed）能在
// mock 下驱动前端 onNotesChanged → loadNotes 刷新。emit 供 tauri-core mock 触发。
const listeners = new Map<string, Set<() => void>>();

export async function listen(event: string, cb: () => void): Promise<() => void> {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(cb);
  return () => {
    listeners.get(event)?.delete(cb);
  };
}

/** 触发事件（供 mock 的后端命令在需要刷新时调用）。 */
export function emit(event: string): void {
  listeners.get(event)?.forEach((cb) => cb());
}
