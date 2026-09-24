import {
  computed,
  effect,
  flush,
  onCleanup,
  provideContext,
  signal,
  untrack,
  useContext,
  type ContextKey,
  type Getter,
} from "@rezejs/signals";

import { Loading as LoadingFeature } from "./features";
import { isServerRender } from "./hydration";
import type { JSX } from "./jsx";

export interface LoadingProps {
  fallback?: JSX.Element;
  /**
   * Scopes the fallback: pending work shows it only when `on` changed since the children were
   * last shown; otherwise the children stay, as in a transition.
   */
  on?: unknown;
  children: JSX.Element;
}

/** A `Loading` as pending work, `Reveal` and transitions see it. */
export interface LoadingBoundary {
  retain(): void;
  release(): void;
  /** Whether it shows its children now (tracked). */
  isShowingContent(): boolean;
  /** Whether it has shown its children at least once. */
  hasShownContent: boolean;
  /** Set by an enclosing `Reveal`: whether the children may show once ready (tracked). */
  isReleased: Getter<boolean>;
  /** Whether nothing inside is pending (tracked). */
  isReady(): boolean;
}

export const LoadingContext: ContextKey<LoadingBoundary | undefined> = {
  id: Symbol("loading"),
  defaultValue: undefined,
};

interface Transition {
  holding: Set<LoadingBoundary>;
  finish(): void;
}

let activeTransition: Transition | undefined;

/**
 * Registers pending work with the nearest `Loading`: while `isPending()` is true the boundary
 * shows its fallback, unless it already showed its children and a transition or its `on` keeps
 * them. Called by async components.
 */
export function trackPending(isPending: () => boolean): void {
  if (!LoadingFeature) return;
  const boundary = useContext(LoadingContext);
  if (boundary === undefined) return;
  effect(() => {
    if (!isPending()) return;
    const transition = activeTransition;
    if (transition !== undefined && boundary.hasShownContent) {
      transition.holding.add(boundary);
      onCleanup(() => {
        transition.holding.delete(boundary);
        if (transition.holding.size === 0) transition.finish();
      });
      return;
    }
    boundary.retain();
    onCleanup(() => boundary.release());
  });
}

/**
 * Shows `fallback` while async work inside is pending, then `children`. The children are created
 * once, up front, and kept while the fallback shows; they are detached from the document
 * meanwhile. No wrapper element is rendered. The server always renders the children.
 */
export function Loading(props: LoadingProps): JSX.Element {
  if (isServerRender()) return props.children;
  const [pendingCount, setPendingCount] = signal(0);
  const [isReleased, setReleased] = signal(true);
  let shownOn: unknown;
  const boundary: LoadingBoundary = {
    retain: () => void setPendingCount((count) => count + 1),
    release: () => void setPendingCount((count) => count - 1),
    isShowingContent: () => showsContent(),
    hasShownContent: false,
    isReleased,
    isReady: () => pendingCount() === 0,
  };
  const reveal = useContext(RevealContext);
  if (reveal !== undefined) setReleased(false);
  reveal?.register(boundary, setReleased);
  const children = provideContext(LoadingContext, boundary, () => props.children);
  const showsContent = computed(() => {
    const isPending = pendingCount() > 0;
    if (!isReleased()) return false;
    if (!isPending) return true;
    return (
      boundary.hasShownContent &&
      "on" in props &&
      Object.is(
        untrack(() => props.on),
        shownOn,
      )
    );
  });
  return computed(() => {
    if (showsContent()) {
      boundary.hasShownContent = true;
      shownOn = untrack(() => props.on);
      return children;
    }
    if (reveal?.suppressesFallback(boundary)) return undefined;
    return untrack(() => props.fallback);
  });
}

/** A `Reveal` group as the `Loading` boundaries inside it see it. */
export interface RevealGroup {
  register(boundary: LoadingBoundary, setReleased: (isReleased: boolean) => void): void;
  suppressesFallback(boundary: LoadingBoundary): boolean;
}

export const RevealContext: ContextKey<RevealGroup | undefined> = {
  id: Symbol("reveal"),
  defaultValue: undefined,
};

/**
 * Runs `fn` as a transition: boundaries that already show their children keep them while the
 * work `fn` starts is pending, instead of falling back. Resolves once that work settled.
 */
export function startTransition(fn: () => void): Promise<void> {
  return runTransition(fn, () => {});
}

function runTransition(
  fn: () => void,
  onPendingChange: (isPending: boolean) => void,
): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  let isFinished = false;
  const transition: Transition = {
    holding: new Set(),
    finish() {
      if (isFinished) return;
      isFinished = true;
      onPendingChange(false);
      resolve();
    },
  };
  const previous = activeTransition;
  activeTransition = transition;
  onPendingChange(true);
  try {
    fn();
    flush();
  } finally {
    activeTransition = previous;
  }
  if (transition.holding.size === 0) transition.finish();
  return promise;
}

/** `[isPending, start]`: `start(fn)` is `startTransition(fn)`, with `isPending()` true until it settles. */
export function useTransition(): [
  isPending: Getter<boolean>,
  start: (fn: () => void) => Promise<void>,
] {
  const [isPending, setPending] = signal(false);
  let running = 0;
  const start = (fn: () => void): Promise<void> =>
    runTransition(fn, (pending) => {
      running += pending ? 1 : -1;
      setPending(running > 0);
    });
  return [isPending, start];
}
