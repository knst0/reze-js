import { computed, onCleanup, root, signal, untrack, type Setter } from "@rezejs/signals";

import type { JSX } from "./jsx";

interface Rendered {
  value: JSX.Element;
  dispose: () => void;
}

class Row<T> {
  declare key: unknown;
  declare index: number;
  declare item: T | undefined;
  declare dispose: () => void;
  declare value: JSX.Element;
  declare setItem: Setter<T> | undefined;
  declare setIndex: Setter<number> | undefined;
  declare next: Row<T> | undefined;

  constructor(key: unknown, index: number, dispose: () => void) {
    this.key = key;
    this.index = index;
    this.item = undefined;
    this.dispose = dispose;
    this.value = undefined;
    this.setItem = undefined;
    this.setIndex = undefined;
    this.next = undefined;
  }

  update(item: T, index: number): void {
    if (this.item !== item) {
      this.item = item;
      if (this.setItem !== undefined) {
        if (typeof item === "function") {
          this.setItem(() => item);
        } else {
          this.setItem(item);
        }
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
 * Identity list: one row per item (`===`), each in its own root that is disposed when the item
 * leaves. A kept row keeps its nodes; `item` is the row's value and `index()` signals only when
 * `map` declares it. `each()`, its `length` and every item are read tracked. `fallback` is shown,
 * in its own root, while the list is empty.
 */
export function list<T>(
  each: () => readonly T[] | null | undefined | false,
  map: (item: T, index: () => number) => JSX.Element,
  fallback?: () => JSX.Element,
  keyed?: true,
): () => JSX.Element;
/**
 * Index list: one row per position, each in its own root. Rows never move; `item()` signals the
 * value at the row's position and `index` is that position. Tracking and `fallback` are shared
 * with the identity list.
 */
export function list<T>(
  each: () => readonly T[] | null | undefined | false,
  map: (item: () => T, index: number) => JSX.Element,
  fallback: (() => JSX.Element) | undefined,
  keyed: false,
): () => JSX.Element;
/**
 * Keyed list: one row per `keyed(item)`, each in its own root that is disposed when the key
 * leaves. A kept row keeps its nodes and takes the new item in place; both `item()` and
 * `index()` are signals, the latter only when `map` declares it. Tracking and `fallback` are
 * shared with the identity list.
 */
export function list<T>(
  each: () => readonly T[] | null | undefined | false,
  map: (item: () => T, index: () => number) => JSX.Element,
  fallback: (() => JSX.Element) | undefined,
  keyed: (item: T) => unknown,
): () => JSX.Element;
export function list<T>(
  each: () => readonly T[] | null | undefined | false,
  map: (item: never, index: never) => JSX.Element,
  fallback?: () => JSX.Element,
  keyed?: boolean | ((item: T) => unknown),
): () => JSX.Element {
  if (keyed === false) {
    return indexed(each, map as (item: () => T, index: number) => JSX.Element, fallback);
  }
  return keyedList(
    each,
    map as (item: T | (() => T), index: () => number) => JSX.Element,
    fallback,
    typeof keyed === "function" ? keyed : undefined,
  );
}

function keyedList<T>(
  each: () => readonly T[] | null | undefined | false,
  map: (item: T | (() => T), index: () => number) => JSX.Element,
  fallback: (() => JSX.Element) | undefined,
  key: ((item: T) => unknown) | undefined,
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
      let getIndex: () => number;
      if (hasIndex) {
        [getIndex, row.setIndex] = signal(index);
      } else {
        getIndex = () => row.index;
      }
      if (key === undefined) {
        row.value = map(item, getIndex);
      } else {
        const [getItem, setItem] = signal<T>(item);
        row.setItem = setItem;
        row.value = map(getItem, getIndex);
      }
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

function indexed<T>(
  each: () => readonly T[] | null | undefined | false,
  map: (item: () => T, index: number) => JSX.Element,
  fallback: (() => JSX.Element) | undefined,
): () => JSX.Element {
  let items: readonly T[] = Empty;
  let rows: Row<T>[] = [];
  let shownFallback: Rendered | undefined;

  onCleanup(() => {
    for (let i = 0; i < rows.length; i++) {
      rows[i]!.dispose();
    }
    shownFallback?.dispose();
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
    while (rows.length < n) {
      const index = rows.length;
      rows.push(
        root((dispose) => {
          const row = new Row<T>(index, index, dispose);
          const [getItem, setItem] = signal<T>(items[index]!);
          row.setItem = setItem;
          row.value = map(getItem, index);
          return row;
        }),
      );
    }
    while (rows.length > n) {
      rows.pop()!.dispose();
    }
    const out: JSX.Element[] = [];
    for (let i = 0; i < n; i++) {
      const row = rows[i]!;
      row.update(items[i]!, i);
      out.push(row.value);
    }
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
