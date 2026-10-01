import { computed, onCleanup, root, signal, untrack, type Setter } from "@rezejs/signals";

import type { JSX } from "./jsx";

interface Rendered {
  value: JSX.Element;
  dispose: () => void;
}

class Row<T> {
  declare key: unknown;
  declare index: number;
  declare dispose: () => void;
  declare value: JSX.Element;
  declare setItem: Setter<T> | undefined;
  declare setIndex: Setter<number> | undefined;
  declare next: Row<T> | undefined;

  constructor(key: unknown, index: number, dispose: () => void) {
    this.key = key;
    this.index = index;
    this.dispose = dispose;
    this.value = undefined;
    this.setItem = undefined;
    this.setIndex = undefined;
    this.next = undefined;
  }

  update(item: T, index: number): void {
    if (this.setItem !== undefined) {
      if (typeof item === "function") {
        this.setItem(() => item);
      } else {
        this.setItem(item);
      }
    }
    if (this.index !== index) {
      this.index = index;
      this.setIndex?.(index);
    }
  }
}

const Empty: readonly never[] = [];

/**
 * Keyed list: one row per key (`key(item)`, or the item itself), each in its own root that is disposed when the key
 * leaves. A kept row keeps its nodes; `item()` is a signal only with `key` and `index()` only when `map` declares it.
 * `each()`, its `length` and every item are read tracked. `fallback` is shown, in its own root, while the list is
 * empty.
 */
export function list<T>(
  each: () => readonly T[] | null | undefined | false,
  map: (item: () => T, index: () => number) => JSX.Element,
  fallback?: () => JSX.Element,
  key?: (item: T) => unknown,
): () => JSX.Element {
  const hasIndex = map.length > 1;
  let items: readonly T[] = Empty;
  let rows: Row<T>[] = [];
  let shownFallback: Rendered | undefined;

  onCleanup(() => {
    for (let i = 0; i < rows.length; i++) {
      rows[i]!.dispose();
    }
    shownFallback?.dispose();
  });

  const createRow = (item: T, rowKey: unknown, index: number): Row<T> =>
    root((dispose) => {
      const row = new Row<T>(rowKey, index, dispose);
      let getItem: () => T;
      let getIndex: () => number;
      if (key === undefined) {
        getItem = () => item;
      } else {
        [getItem, row.setItem] = signal(item);
      }
      if (hasIndex) {
        [getIndex, row.setIndex] = signal(index);
      } else {
        getIndex = () => row.index;
      }
      row.value = map(getItem, getIndex);
      return row;
    });

  const update = (): JSX.Element => {
    const n = items.length;
    if (n === 0) {
      for (let i = 0; i < rows.length; i++) {
        rows[i]!.dispose();
      }
      rows = [];
      if (fallback === undefined) {
        return undefined;
      }
      return (shownFallback ??= root((dispose) => ({ value: fallback(), dispose }))).value;
    }
    if (shownFallback !== undefined) {
      shownFallback.dispose();
      shownFallback = undefined;
    }
    const nextRows: Row<T>[] = [];
    const out: JSX.Element[] = [];
    let start = 0;
    for (const end = Math.min(n, rows.length); start < end; start++) {
      const item = items[start]!;
      const row = rows[start]!;
      if (row.key !== (key === undefined ? item : key(item))) {
        break;
      }
      row.update(item, start);
      nextRows.push(row);
      out.push(row.value);
    }
    let byKey: Map<unknown, Row<T>> | undefined;
    if (start < rows.length) {
      byKey = new Map();
      for (let i = rows.length; i-- > start;) {
        const row = rows[i]!;
        row.next = byKey.get(row.key);
        byKey.set(row.key, row);
      }
    }
    for (let i = start; i < n; i++) {
      const item = items[i]!;
      const rowKey = key === undefined ? item : key(item);
      let row = byKey?.get(rowKey);
      if (row === undefined) {
        row = createRow(item, rowKey, i);
      } else {
        if (row.next === undefined) {
          byKey!.delete(rowKey);
        } else {
          byKey!.set(rowKey, row.next);
          row.next = undefined;
        }
        row.update(item, i);
      }
      nextRows.push(row);
      out.push(row.value);
    }
    if (byKey !== undefined) {
      for (const first of byKey.values()) {
        for (let row: Row<T> | undefined = first; row !== undefined; row = row.next) {
          row.dispose();
        }
      }
    }
    rows = nextRows;
    return out;
  };

  return computed(() => {
    const next = each() || Empty;
    for (let i = 0, n = next.length; i < n; i++) {
      void next[i];
    }
    items = next;
    return untrack(update);
  });
}
