import { useCallback, useState } from "react";

export const SIDEBAR_WIDTH = 228;
export const SIDEBAR_COLLAPSED_WIDTH = 64;
export const LIST_DEFAULT_WIDTH = 360;
export const LIST_MIN_WIDTH = 260;
export const LIST_MAX_WIDTH = 720;
/** 拖动列表宽度时，阅读区至少保留的宽度 */
export const READER_MIN_WIDTH = 360;

/** 邮件列表宽度：限制在最小 / 最大值之间，并给阅读区留出空间 */
export function clampListWidth(width: number, windowWidth: number, sidebarWidth: number): number {
  const room = windowWidth - sidebarWidth - READER_MIN_WIDTH;
  const max = Math.max(LIST_MIN_WIDTH, Math.min(LIST_MAX_WIDTH, room));
  if (!Number.isFinite(width)) return Math.min(LIST_DEFAULT_WIDTH, max);
  return Math.round(Math.min(max, Math.max(LIST_MIN_WIDTH, width)));
}

/** 读取本地保存的 JSON 值；类型不匹配或损坏时用默认值 */
export function parseStored<T>(raw: string | null, fallback: T): T {
  if (raw === null) return fallback;
  try {
    const value = JSON.parse(raw);
    return typeof value === typeof fallback ? (value as T) : fallback;
  } catch {
    return fallback;
  }
}

/** 保存在 localStorage 里的界面状态（侧栏折叠、列宽等），不属于用户设置 */
export function usePersistentState<T>(key: string, fallback: T) {
  const storageKey = `mailbox.layout.${key}`;
  const [value, setValue] = useState<T>(() => parseStored(localStorage.getItem(storageKey), fallback));
  const update = useCallback(
    (next: T | ((prev: T) => T)) => {
      setValue((prev) => {
        const resolved = typeof next === "function" ? (next as (p: T) => T)(prev) : next;
        localStorage.setItem(storageKey, JSON.stringify(resolved));
        return resolved;
      });
    },
    [storageKey],
  );
  return [value, update] as const;
}
