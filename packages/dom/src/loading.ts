import { asyncComputed, type AsyncContext, type Boundary, boundary, computed, untrack } from "@rezejs/signals";
import { renderEffect } from "@rezejs/signals/render";

import type { JSX } from "./jsx";

/** Resolves a built view eagerly: calls thunks until a non-function remains, flattens arrays resolving each item. */
function resolve(value: unknown): unknown {
  while (typeof value === "function") {
    value = (value as () => unknown)();
  }
  if (Array.isArray(value)) {
    const out: unknown[] = [];
    for (const item of value) {
      const resolved = resolve(item);
      if (Array.isArray(resolved)) {
        out.push(...resolved);
      } else {
        out.push(resolved);
      }
    }
    return out;
  }
  return value;
}

/** Builds `build()` in a boundary and resolves it eagerly, kept watched, so every lazy child exists and reads now. */
function prepare(build: () => JSX.Element): [() => JSX.Element, Boundary] {
  return boundary(() => {
    const view = build();
    const resolved = computed(() => resolve(view) as JSX.Element);
    renderEffect(resolved);
    return resolved;
  });
}

/** Runtime of `<Loading>`: shows `fallback` until nothing `children` built has a pending first load, then the children for good. */
export function loading(children: () => JSX.Element, fallback?: () => JSX.Element): () => JSX.Element {
  const [content, scope] = prepare(children);
  const isShown = computed<boolean>((wasShown) => wasShown === true || !scope.isPending());
  return computed(() => (isShown() ? content() : fallback === undefined ? undefined : untrack(fallback)));
}

/**
 * The child of an async component: `body` runs once, untracked, after the first run of `load` resolves, and reads
 * the resolved values through `values()`, so a re-run updates them in place. Renders `undefined` until then and
 * throws the rejection of the latest run into the surrounding `catchError`.
 */
export function asyncComponent<V extends unknown[], R>(
  load: (c: AsyncContext) => PromiseLike<V>,
  body: (values: () => V) => R,
): () => R | undefined {
  const step = asyncComputed(load);
  const values = (): V => step.value()!;
  const isLoaded = computed(() => step.value() !== undefined);
  const view = computed(() => (isLoaded() ? untrack(() => body(values)) : undefined));
  return computed(() => {
    const current = view();
    const error = step.error();
    if (error !== undefined) {
      throw error;
    }
    return current;
  });
}
