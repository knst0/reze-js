import { computed, untrack } from "@rezejs/signals";

import type { JSX } from "./jsx";

/**
 * Replaces a flow's shown side: builds `build(key())` untracked whenever `key()` changes.
 */
export type Swap = <K>(key: () => K, build: (key: K) => JSX.Element) => () => JSX.Element;

export function swapNow<K>(key: () => K, build: (key: K) => JSX.Element): () => JSX.Element {
  return computed(() => {
    const current = key();
    return untrack(() => build(current));
  });
}

let swap: Swap = swapNow;

/**
 * Sets how `branch` and `choose` swap sides; `loading` installs the holding swap.
 */
export function setSwap(next: Swap): void {
  swap = next;
}

/**
 * Shows `child` while `when()` is truthy, else `fallback`. The shown side is rebuilt, untracked, only when truthiness
 * flips; `child` receives a cached getter of `when()`.
 */
export function branch<T>(when: () => T, child: (value: () => T) => JSX.Element, fallback?: () => JSX.Element): () => JSX.Element {
  const isShown = computed(() => !!when());
  let value: (() => T) | undefined;
  const read = (): T => (value ??= computed(when))();
  return swap(isShown, (shown) => (shown ? child(read) : fallback === undefined ? undefined : fallback()));
}

/**
 * Shows `children[i]` for the first truthy `whens[i]()`, else `fallback`. The shown side is rebuilt, untracked, only
 * when that index changes; `children[i]` receives `whens[i]` as its value getter.
 */
export function choose(
  whens: readonly (() => unknown)[],
  children: readonly ((value: () => unknown) => JSX.Element)[],
  fallback?: () => JSX.Element,
): () => JSX.Element {
  const index = computed(() => {
    for (let i = 0; i < whens.length; i++) {
      if (whens[i]!()) {
        return i;
      }
    }
    return -1;
  });
  return swap(index, (i) => (i < 0 ? (fallback === undefined ? undefined : fallback()) : children[i]!(whens[i]!)));
}
