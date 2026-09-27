export interface HistoryEntry {
  /** Router path: pathname, search and hash. */
  readonly path: string;
  readonly state: unknown;
  /** Position in the session, so the router can tell back from forward. */
  readonly index: number;
}

export interface RouterHistory {
  get(): HistoryEntry;
  push(path: string, state: unknown): void;
  replace(path: string, state: unknown): void;
  go(delta: number): void;
  /** Called once per entry change the router did not cause (back/forward, go). */
  listen(listener: (entry: HistoryEntry) => void): () => void;
  /** Router path for a clicked same-origin anchor URL, or `undefined` to leave it to the browser. */
  resolve(url: URL): string | undefined;
  /** Whether the router manages `window` scroll for this history. */
  readonly scroll: boolean;
}

interface StoredState {
  readonly reze: 1;
  readonly index: number;
  readonly state: unknown;
}

function isStored(value: unknown): value is StoredState {
  return typeof value === "object" && value !== null && (value as StoredState).reze === 1;
}

function windowHistory(read: () => string, toUrl: (path: string) => string, resolve: (url: URL) => string | undefined): RouterHistory {
  const history = window.history;
  if (!isStored(history.state)) {
    history.replaceState({ reze: 1, index: 0, state: history.state }, "", location.href);
  }
  const entryOf = (stored: unknown): HistoryEntry => {
    const isOwn = isStored(stored);
    return { path: read(), state: isOwn ? stored.state : undefined, index: isOwn ? stored.index : 0 };
  };
  return {
    get: () => entryOf(history.state),
    push(path, state) {
      const index = entryOf(history.state).index + 1;
      history.pushState({ reze: 1, index, state } satisfies StoredState, "", toUrl(path));
    },
    replace(path, state) {
      const index = entryOf(history.state).index;
      history.replaceState({ reze: 1, index, state } satisfies StoredState, "", toUrl(path));
    },
    go: (delta) => history.go(delta),
    listen(listener) {
      const onPop = (event: PopStateEvent): void => listener(entryOf(event.state));
      window.addEventListener("popstate", onPop);
      return () => window.removeEventListener("popstate", onPop);
    },
    resolve,
    scroll: true,
  };
}

/** History over `window.location` paths, served under `base` (e.g. Vite's `import.meta.env.BASE_URL`). */
export function createBrowserHistory(base = ""): RouterHistory {
  const prefix = ("/" + base).replace(/\/+/g, "/").replace(/\/$/, "");
  const strip = (pathname: string): string | undefined => {
    if (pathname === prefix) return "/";
    return pathname.startsWith(prefix + "/") ? pathname.slice(prefix.length) : undefined;
  };
  return windowHistory(
    () => (strip(location.pathname) ?? location.pathname) + location.search + location.hash,
    (path) => prefix + path,
    (url) => {
      const pathname = strip(url.pathname);
      return pathname === undefined ? undefined : pathname + url.search + url.hash;
    },
  );
}

/** History kept in `location.hash` (`#/path`), for hosts that cannot rewrite unknown paths to the app. */
export function createHashHistory(): RouterHistory {
  return windowHistory(
    () => location.hash.slice(1) || "/",
    (path) => "#" + path,
    (url) =>
      url.pathname + url.search === location.pathname + location.search && url.hash.startsWith("#/") ? url.hash.slice(1) : undefined,
  );
}

/** In-memory history for tests and embedded routers; never touches `window`. */
export function createMemoryHistory(initial = "/"): RouterHistory {
  const entries: HistoryEntry[] = [{ path: initial, state: undefined, index: 0 }];
  let index = 0;
  const listeners = new Set<(entry: HistoryEntry) => void>();
  return {
    get: () => entries[index]!,
    push(path, state) {
      index++;
      entries.length = index;
      entries.push({ path, state, index });
    },
    replace(path, state) {
      entries[index] = { path, state, index };
    },
    go(delta) {
      const next = Math.max(0, Math.min(entries.length - 1, index + delta));
      if (next === index) return;
      index = next;
      for (const listener of listeners) listener(entries[index]!);
    },
    listen(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    resolve: (url) => url.pathname + url.search + url.hash,
    scroll: false,
  };
}
