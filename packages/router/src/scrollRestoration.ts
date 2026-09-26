// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { effect, onCleanup } from "@rezejs/signals";

import { bindEvent, saveCurrentDepth, type DepthState, type RouterHistory } from "./history";
import type { Location } from "./types";

const StorageKey = "rezejs-router:scroll";

export interface ScrollRestoration {
  onPop(): void;
  onPush(): void;
  create(router: { location: Location; isRouting: () => boolean }): void;
}

/**
 * Back/forward scroll restoration: offsets are captured on scroll, keyed by the entry's
 * `_depth`, persisted to `sessionStorage` on `pagehide`, and restored in one scroll once the
 * traversal settled (the browser's own heuristic clamps offsets while the next page renders).
 */
export function createScrollRestoration(): ScrollRestoration {
  window.history.scrollRestoration = "manual";
  saveCurrentDepth();
  let positions: Record<number, number> = {};
  try {
    positions = JSON.parse(sessionStorage.getItem(StorageKey) ?? "{}") ?? {};
  } catch {
    positions = {};
  }
  const currentDepth = (): number | undefined =>
    (window.history.state as DepthState | null)?._depth;
  let isProgrammatic = false;
  let pendingDepth: number | undefined;
  const unbind = [
    bindEvent(window, "scroll", () => {
      const depth = currentDepth();
      if (depth != null) positions[depth] = window.scrollY;
      if (!isProgrammatic) pendingDepth = undefined;
    }),
    bindEvent(window, "pagehide", () => {
      try {
        sessionStorage.setItem(StorageKey, JSON.stringify(positions));
      } catch {
        return;
      }
    }),
  ];
  const restore = (): void => {
    if (pendingDepth == null) return;
    const offset = positions[pendingDepth];
    pendingDepth = undefined;
    if (offset == null) return;
    isProgrammatic = true;
    window.scrollTo(0, offset);
    isProgrammatic = false;
  };
  return {
    onPop() {
      pendingDepth = currentDepth();
    },
    onPush() {
      const depth = currentDepth();
      if (depth == null) return;
      for (const key in positions) if (+key >= depth) delete positions[key];
    },
    create(router) {
      const [navigation] = (performance.getEntriesByType?.("navigation") ??
        []) as PerformanceNavigationTiming[];
      if (navigation && navigation.type !== "navigate") pendingDepth = currentDepth();
      effect(() => {
        void (router.location.pathname + router.location.search + router.location.hash);
        if (!router.isRouting()) queueMicrotask(restore);
      });
      onCleanup(() => unbind.forEach((unsubscribe) => unsubscribe()));
    },
  };
}

export function withScrollRestoration(
  history: RouterHistory,
  restoration: ScrollRestoration,
): RouterHistory {
  return {
    ...history,
    set(next) {
      history.set(next);
      if (!next.replace) restoration.onPush();
    },
    init:
      history.init &&
      ((notify) =>
        history.init!((value) => {
          restoration.onPop();
          notify(value);
        })),
  };
}
