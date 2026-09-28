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

/**
 * `read` returns the router path of the current URL, or `undefined` when the URL is not one (an in-page `#anchor` under hash
 * history), in which case the entry change is left to the browser and the router keeps its path. An entry without router
 * state is adopted with the index it most likely has: `history.length - 1` for a fresh document, one past the current entry
 * on `popstate` (a hash edited in the address bar).
 */
function windowHistory(
  read: () => string | undefined,
  toUrl: (path: string) => string,
  resolve: (url: URL) => string | undefined,
): RouterHistory {
  const history = window.history;
  const adopt = (index: number): StoredState => {
    const existing: unknown = history.state;
    if (isStored(existing)) return existing;
    const stored: StoredState = { reze: 1, index, state: existing };
    history.replaceState(stored, "", location.href);
    return stored;
  };
  let current = adopt(history.length - 1);
  let path = read() ?? "/";
  const entry = (): HistoryEntry => ({ path, state: current.state, index: current.index });
  return {
    get: entry,
    push(next, state) {
      current = { reze: 1, index: current.index + 1, state };
      history.pushState(current, "", toUrl(next));
      path = next;
    },
    replace(next, state) {
      current = { reze: 1, index: current.index, state };
      history.replaceState(current, "", toUrl(next));
      path = next;
    },
    go: (delta) => history.go(delta),
    listen(listener) {
      const onPop = (): void => {
        current = adopt(current.index + 1);
        const next = read();
        if (next === undefined) return;
        path = next;
        listener(entry());
      };
      window.addEventListener("popstate", onPop);
      return () => window.removeEventListener("popstate", onPop);
    },
    resolve,
    scroll: true,
  };
}

const UrlOrRelative = /^(?:\.|\/\/|[a-z][a-z\d+.-]*:)/i;

/** `base` (e.g. Vite's `import.meta.env.BASE_URL`) as a path prefix without a trailing slash; `""` for the root and for relative or URL bases, which name no path. */
export function routerBase(base: string): string {
  return UrlOrRelative.test(base) ? "" : ("/" + base).replace(/\/+/g, "/").replace(/\/$/, "");
}

/** History over `window.location` paths, served under `base` (e.g. Vite's `import.meta.env.BASE_URL`; a relative base means the root). */
export function createBrowserHistory(base = ""): RouterHistory {
  const prefix = routerBase(base);
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

/** History kept in `location.hash` (`#/path`), for hosts that cannot rewrite unknown paths to the app. Links to routes are written `href="#/path"` (the routes plugin's `history: "hash"` types them); other hashes stay in-page anchors. */
export function createHashHistory(): RouterHistory {
  return windowHistory(
    () => {
      const hash = location.hash;
      if (hash === "") return "/";
      return hash.startsWith("#/") ? hash.slice(1) : undefined;
    },
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
