import {
  type AsyncComputed,
  asyncComputed,
  type AsyncContext,
  computed,
  type ContextKey,
  effect,
  provideContext,
  signal,
  untrack,
  useContext,
} from "@rezejs/signals";

import type { JSX } from "./jsx";

const loadingContext: ContextKey<((delta: number) => void) | undefined> = { id: Symbol("loading"), defaultValue: undefined };

export interface LoadingProps {
  fallback?: JSX.Element;
  children?: JSX.Element;
}

/**
 * Shows `fallback` instead of `children` until every async component created while `children` are built has rendered
 * its first content, then shows `children` for good: reloads keep the content on screen. `children` are created once;
 * async components inside a lazily built child such as `<Show>` or `<For>` are created only after the content shows, so
 * they render in place without the fallback.
 */
export function Loading(props: LoadingProps): JSX.Element {
  const [pending, setPending] = signal(0);
  const isLoading = computed(() => pending() > 0);
  let hasShownContent = false;
  return provideContext(
    loadingContext,
    (delta) => setPending((count) => count + delta),
    () => {
      const children = props.children;
      return computed(() => {
        if (hasShownContent || !isLoading()) {
          hasShownContent = true;
          return children;
        }
        return untrack(() => props.fallback);
      });
    },
  );
}

function releaseOnFirstSettle(track: (delta: number) => void, step: AsyncComputed<unknown>): void {
  track(1);
  let isWaiting = true;
  effect(() => {
    if (isWaiting && (step.value() !== undefined || step.error() !== undefined)) {
      isWaiting = false;
      track(-1);
    }
  });
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
  const track = useContext(loadingContext);
  const step = asyncComputed(load);
  if (track !== undefined) {
    releaseOnFirstSettle(track, step);
  }
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
