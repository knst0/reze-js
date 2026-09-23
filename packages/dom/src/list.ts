import {
  computed,
  onCleanup,
  root,
  signal,
  untrack,
  type Getter,
  type Setter,
} from "@rezejs/signals";

import type { JSX } from "./jsx";

export interface ForProps<T> {
  each: readonly T[] | null | undefined | false;
  /** Row identity; defaults to the item itself. Read once. */
  key?: (item: T) => unknown;
  fallback?: JSX.Element;
  /** Read once. Declaring `index` costs one signal per row; omitting it costs none. */
  children: (item: Getter<T>, index: Getter<number>) => JSX.Element;
}

class Row<T> {
  key: unknown;
  value: JSX.Element = undefined;
  dispose: () => void;
  /** Only with `key`: the same key may arrive with a new item. */
  setItem: Setter<T> | undefined = undefined;
  /** Only when the mapper declares `index`. */
  setIndex: Setter<number> | undefined = undefined;
  index: number;
  /** Next old row with the same key, while matching. */
  next: Row<T> | undefined = undefined;

  constructor(key: unknown, index: number, dispose: () => void) {
    this.key = key;
    this.index = index;
    this.dispose = dispose;
  }

  /** Moves a kept row to `index` with `item`; getters notify only on change. */
  update(item: T, index: number): void {
    this.setItem?.(() => item);
    if (this.index !== index) {
      this.index = index;
      this.setIndex?.(index);
    }
  }
}

/**
 * Keyed list. A row whose key survives an update keeps its DOM and owner; `item()` and `index()`
 * update in place. Each row owns its own root, disposed when the row leaves the list.
 */
export function For<T>(props: ForProps<T>): JSX.Element {
  const key = props.key;
  const mapFn = props.children;
  let rows: Row<T>[] = [];
  let fallback: { value: JSX.Element; dispose: () => void } | undefined;

  const createRow = (item: T, k: unknown, index: number): Row<T> =>
    root((dispose) => {
      const row = new Row<T>(k, index, dispose);
      let getItem: Getter<T> = () => item;
      let getIndex: Getter<number> = () => row.index;
      if (key) [getItem, row.setItem] = signal(item);
      if (mapFn.length > 1) [getIndex, row.setIndex] = signal(index);
      row.value = mapFn(getItem, getIndex);
      return row;
    });

  onCleanup(() => {
    for (const row of rows) row.dispose();
    fallback?.dispose();
  });

  return computed(() => {
    const items = props.each || [];
    return untrack(() => {
      const n = items.length;
      if (n === 0) {
        for (const row of rows) row.dispose();
        rows = [];
        fallback ??= root((dispose) => ({ value: props.fallback, dispose }));
        return fallback.value;
      }
      if (fallback) {
        fallback.dispose();
        fallback = undefined;
      }

      // Filled strictly in index order, so plain arrays stay packed.
      const next: Row<T>[] = [];
      const out: JSX.Element[] = [];
      // Common prefix: the usual append/update case needs no key map.
      let start = 0;
      for (const end = Math.min(n, rows.length); start < end; start++) {
        const item = items[start]!;
        const row = rows[start]!;
        if (row.key !== (key ? key(item) : item)) break;
        row.update(item, start);
        next[start] = row;
        out[start] = row.value;
      }

      // Old rows by key; duplicates chain through `next`, earliest first.
      const byKey = new Map<unknown, Row<T>>();
      for (let i = rows.length; i-- > start;) {
        const row = rows[i]!;
        row.next = byKey.get(row.key);
        byKey.set(row.key, row);
      }
      for (let i = start; i < n; i++) {
        const item = items[i]!;
        const k = key ? key(item) : item;
        let row = byKey.get(k);
        if (row) {
          if (row.next) byKey.set(k, row.next);
          else byKey.delete(k);
          row.next = undefined;
          row.update(item, i);
        } else {
          row = createRow(item, k, i);
        }
        next[i] = row;
        out[i] = row.value;
      }
      for (const row of byKey.values()) {
        for (let r: Row<T> | undefined = row; r; r = r.next) r.dispose();
      }
      rows = next;
      return out;
    });
  });
}
