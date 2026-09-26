// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import type { BeforeLeaveSlot, LocationChange, RouterUtils } from "./types";

export interface RouterHistory {
  get(): string | LocationChange;
  set(next: LocationChange): void;
  /** Subscribes to location changes the adapter did not write; returns the unsubscribe. */
  init?(notify: (value?: string | LocationChange) => void): () => void;
  utils?: Partial<RouterUtils>;
}

export interface MemoryHistoryAdapter extends RouterHistory {
  get(): string;
  go(delta: number): void;
  back(): void;
  forward(): void;
  listen(listener: (value: string) => void): () => void;
}

export interface DepthState {
  _depth?: number;
}

export function bindEvent(target: EventTarget, type: string, handler: EventListener): () => void {
  target.addEventListener(type, handler);
  return () => target.removeEventListener(type, handler);
}

let depth: number | undefined;

/** Stamps the current entry with its index, so a traversal's delta blocks exactly. */
export function saveCurrentDepth(): void {
  const state = window.history.state as DepthState | null;
  if (!state || state._depth == null) {
    window.history.replaceState({ ...state, _depth: window.history.length - 1 }, "");
  }
  depth = (window.history.state as DepthState)._depth;
}

function keepDepth(state: unknown): DepthState {
  return {
    ...(state as object),
    _depth: (window.history.state as DepthState | null)?._depth,
  };
}

function notifyIfNotBlocked(notify: () => void, isBlocked: (delta: number) => boolean) {
  let isReverting = false;
  return () => {
    const previousDepth = depth;
    saveCurrentDepth();
    const delta = previousDepth == null ? null : depth! - previousDepth;
    if (isReverting) {
      isReverting = false;
      return;
    }
    if (delta && isBlocked(delta)) {
      isReverting = true;
      window.history.go(-delta);
    } else {
      notify();
    }
  };
}

function scrollToHash(hash: string, isFallbackTop?: boolean): void {
  const element = hash ? document.getElementById(hash) : null;
  if (element) element.scrollIntoView();
  else if (isFallbackTop) window.scrollTo(0, 0);
}

/** `window.location` with `pushState`/`replaceState` and `popstate`; the client default. */
export function browserHistory(): RouterHistory {
  const getSource = (): LocationChange => {
    const url = window.location.pathname + window.location.search;
    const state = window.history.state as DepthState | null;
    const isDepthOnly = !!state?._depth && Object.keys(state).length === 1;
    return { value: url + window.location.hash, state: isDepthOnly ? undefined : state };
  };
  const beforeLeave: BeforeLeaveSlot = {};
  if (typeof window !== "undefined") saveCurrentDepth();
  return {
    get: getSource,
    set({ value, replace, scroll, state }) {
      if (replace) window.history.replaceState(keepDepth(state), "", value);
      else window.history.pushState(state, "", value);
      scrollToHash(decodeURIComponent(window.location.hash.slice(1)), scroll);
      saveCurrentDepth();
    },
    init: (notify) =>
      bindEvent(
        window,
        "popstate",
        notifyIfNotBlocked(notify, (delta) => {
          const guard = beforeLeave.current;
          if (!guard) return false;
          if (delta) return !guard.confirm(delta);
          const source = getSource();
          return !guard.confirm(source.value, { state: source.state });
        }),
      ),
    utils: { go: (delta) => window.history.go(delta), beforeLeave },
  };
}

function hashParser(path: string): string {
  const to = path.replace(/^.*?#/, "");
  if (to.startsWith("/")) return to;
  const [, current = "/"] = window.location.hash.split("#", 2);
  return `${current}#${to}`;
}

/** Keeps the routed path after `#`; `paths` and `useHref` render with the `#` prefix. */
export function hashHistory(): RouterHistory {
  const getSource = (): string => window.location.hash.slice(1);
  const beforeLeave: BeforeLeaveSlot = {};
  if (typeof window !== "undefined") saveCurrentDepth();
  return {
    get: getSource,
    set({ value, replace, scroll, state }) {
      if (replace) window.history.replaceState(keepDepth(state), "", "#" + value);
      else window.history.pushState(state, "", "#" + value);
      const hashIndex = value.indexOf("#");
      scrollToHash(hashIndex >= 0 ? value.slice(hashIndex + 1) : "", scroll);
      saveCurrentDepth();
    },
    init: (notify) =>
      bindEvent(
        window,
        "hashchange",
        notifyIfNotBlocked(notify, (delta) => {
          const guard = beforeLeave.current;
          return !!guard && !guard.confirm(delta && delta < 0 ? delta : getSource());
        }),
      ),
    utils: {
      go: (delta) => window.history.go(delta),
      renderPath: (path) => `#${path}`,
      parsePath: hashParser,
      beforeLeave,
    },
  };
}

/** An in-memory entry stack, for tests and non-browser hosts. `initial` defaults to `/`. */
export function memoryHistory(initial = "/"): MemoryHistoryAdapter {
  const entries = [initial];
  let index = 0;
  const listeners: ((value: string) => void)[] = [];
  const go = (delta: number): void => {
    index = Math.max(0, Math.min(index + delta, entries.length - 1));
    const value = entries[index]!;
    for (const listener of listeners) listener(value);
  };
  const listen = (listener: (value: string) => void): (() => void) => {
    listeners.push(listener);
    return () => void listeners.splice(listeners.indexOf(listener), 1);
  };
  return {
    get: () => entries[index]!,
    set({ value, scroll, replace }) {
      if (replace) {
        entries[index] = value;
      } else {
        entries.splice(index + 1, entries.length - index, value);
        index++;
      }
      for (const listener of listeners) listener(value);
      setTimeout(() => {
        if (scroll) scrollToHash(value.split("#")[1] ?? "", true);
      }, 0);
    },
    back: () => go(-1),
    forward: () => go(1),
    go,
    listen,
    init: listen,
    utils: { go },
  };
}
