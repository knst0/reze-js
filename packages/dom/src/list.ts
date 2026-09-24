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

export interface KeyedForProps<T> {
  each: readonly T[] | null | undefined | false;
  /** Row identity; defaults to the item itself. Read once. `keyed={fn}` is the same. */
  key?: (item: T) => unknown;
  keyed?: true | ((item: T) => unknown);
  fallback?: JSX.Element;
  /** Read once. Declaring `index` costs one signal per row; omitting it costs none. */
  children: (item: Getter<T>, index: Getter<number>) => JSX.Element;
}

export interface IndexedForProps<T> {
  each: readonly T[] | null | undefined | false;
  /** Rows by position: row `i` keeps its DOM while the item at `i` changes. */
  keyed: false;
  fallback?: JSX.Element;
  children: (item: Getter<T>, index: number) => JSX.Element;
}

export type ForProps<T> = KeyedForProps<T> | IndexedForProps<T>;

interface IndexRow<T> {
  value: JSX.Element;
  setItem: (item: T) => void;
  dispose: () => void;
}

/** `<For keyed={false}>`: one row per position, reading its item through a getter. */
function ForByIndex<T>(props: IndexedForProps<T>): JSX.Element {
  const mapFn = props.children;
  let rows: IndexRow<T>[] = [];
  let fallback: { value: JSX.Element; dispose: () => void } | undefined;
  onCleanup(() => {
    for (const row of rows) row.dispose();
    fallback?.dispose();
  });
  return computed(() => {
    const items = props.each || [];
    const n = items.length;
    for (let i = 0; i < n; i++) void items[i];
    return untrack(() => {
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
      for (let i = 0; i < Math.min(n, rows.length); i++) rows[i]!.setItem(items[i]!);
      for (let i = rows.length; i < n; i++) {
        rows.push(
          root((dispose) => {
            const [item, setItem] = signal<T>(items[i]!);
            return { value: mapFn(item, i), setItem: (next) => setItem(() => next), dispose };
          }),
        );
      }
      for (const row of rows.splice(n)) row.dispose();
      return rows.map((row) => row.value);
    });
  });
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
 * update in place. Each row owns its own root, disposed when the row leaves the list. `each`, its
 * `length` and every item are read tracked, so an array mutated in place (a store array) updates too.
 */
export function For<T>(props: KeyedForProps<T>): JSX.Element;
export function For<T>(props: IndexedForProps<T>): JSX.Element;
export function For<T>(props: ForProps<T>): JSX.Element {
  if (props.keyed === false) return ForByIndex(props);
  const key = typeof props.keyed === "function" ? props.keyed : props.key;
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
    const n = items.length;
    for (let i = 0; i < n; i++) void items[i];
    return untrack(() => {
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

export interface RepeatProps {
  count: number;
  /** First index; defaults to `0`. */
  from?: number;
  fallback?: JSX.Element;
  /** A function gets each index; an element is rendered anew for every index. */
  children: ((index: number) => JSX.Element) | JSX.Element;
}

/**
 * One row per index in `from … from + count - 1`. A row keeps its DOM while its index stays in
 * that range; rows leaving it are disposed. `fallback` shows while `count` is not positive.
 */
export function Repeat(props: RepeatProps): JSX.Element {
  let rows = new Map<number, { value: JSX.Element; dispose: () => void }>();
  let fallback: { value: JSX.Element; dispose: () => void } | undefined;
  onCleanup(() => {
    for (const row of rows.values()) row.dispose();
    fallback?.dispose();
  });
  return computed(() => {
    const count = Math.max(0, Math.floor(props.count));
    const from = props.from ?? 0;
    return untrack(() => {
      if (count === 0) {
        for (const row of rows.values()) row.dispose();
        rows.clear();
        fallback ??= root((dispose) => ({ value: props.fallback, dispose }));
        return fallback.value;
      }
      if (fallback) {
        fallback.dispose();
        fallback = undefined;
      }
      const next = new Map<number, { value: JSX.Element; dispose: () => void }>();
      const out: JSX.Element[] = [];
      for (let index = from; index < from + count; index++) {
        const row =
          rows.get(index) ??
          root((dispose) => {
            const children = props.children;
            const value =
              typeof children === "function"
                ? (children as (i: number) => JSX.Element)(index)
                : children;
            return { value, dispose };
          });
        rows.delete(index);
        next.set(index, row);
        out.push(row.value);
      }
      for (const row of rows.values()) row.dispose();
      rows = next;
      return out;
    });
  });
}
