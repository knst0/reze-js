import { computed, untrack } from "@rezejs/signals";

import type { JSX } from "./jsx";

/**
 * Shows `child` while `when()` is truthy, else `fallback`. The shown side is rebuilt, untracked, only when truthiness
 * flips; `child` receives a cached getter of `when()`.
 */
export function branch<T>(when: () => T, child: (value: () => T) => JSX.Element, fallback?: () => JSX.Element): () => JSX.Element {
  const isShown = computed(() => !!when());
  let value: (() => T) | undefined;
  const read = (): T => (value ??= computed(when))();
  return computed(() => (isShown() ? untrack(() => child(read)) : fallback === undefined ? undefined : untrack(fallback)));
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
  return computed(() => {
    const i = index();
    if (i < 0) {
      return fallback === undefined ? undefined : untrack(fallback);
    }
    return untrack(() => children[i]!(whens[i]!));
  });
}
