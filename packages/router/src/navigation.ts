import { flush, untrack, type ContextKey, type Getter, type Owner, type Setter } from "@rezejs/signals";

import type { HistoryEntry, RouterHistory } from "./history";
import { decode, matchBranches, pathKey, type Branch, type BranchMatch, type CompiledRoute } from "./match";
import type { BeforeLeaveEvent, Location, NavigateOptions, Params, PreloadIntent } from "./types";

export interface ActiveMatch {
  readonly route: CompiledRoute;
  readonly path: string;
  readonly params: Params;
  readonly data: unknown;
  readonly error: unknown;
  readonly info: Readonly<Record<string, unknown>> | undefined;
}

/** `initial` restores a position saved by an earlier document, else scrolls to the hash target only. */
export type ScrollMode = "top" | "restore" | "initial" | "none";

/** Per-link state lookups, created on first use under the router's owner. */
export interface LinkSelectors {
  readonly isCurrent: (key: string) => boolean;
  /** Index `d`: whether `key`, of `d` segments, is a strict prefix of the current path. */
  readonly isPrefixAt: ((key: string) => boolean)[];
  readonly isPending: (key: string) => boolean;
  readonly currentKey: Getter<string>;
}

export interface RouterState {
  readonly history: RouterHistory;
  readonly branches: readonly Branch[];
  owner: Owner | undefined;
  /** The committed entry, whose view and scroll position are on screen. */
  entry: HistoryEntry | undefined;
  /** The entry of the latest navigation, ahead of `entry` while its route modules load. */
  target: HistoryEntry | undefined;
  targetLocation: Location | undefined;
  readonly location: Getter<Location>;
  readonly setLocation: Setter<Location>;
  readonly matches: Getter<readonly ActiveMatch[]>;
  readonly setMatches: Setter<readonly ActiveMatch[]>;
  readonly isRouting: Getter<boolean>;
  readonly setIsRouting: Setter<boolean>;
  /** `pathKey` of the pathname a navigation is loading route modules for. */
  readonly pendingKey: Getter<string | undefined>;
  readonly setPendingKey: Setter<string | undefined>;
  links: LinkSelectors | undefined;
  /** Route components being created; a navigation they start commits once they return. */
  renderDepth: number;
  /** The pathname `lastMatch` was matched for; a link click, its hover preload and the navigation share one match. */
  matchedPathname: string | undefined;
  lastMatch: BranchMatch | undefined;
  generation: number;
  readonly leaveListeners: Set<(event: BeforeLeaveEvent) => void>;
  /** `beforeunload` listener, installed while `leaveListeners` is non-empty. */
  readonly onUnload: (event: BeforeUnloadEvent) => void;
  ignorePop: boolean;
  skipNextGuard: boolean;
  /** `[index, scrollX, scrollY]` per slot `index % MaxRestorableEntries`, more than browsers keep in session history (50 in Chrome and Firefox, 100 in WebKit); allocated on first save. */
  positions: Float64Array | undefined;
}

const MaxRestorableEntries = 128;
const PositionsKey = "reze-router:scroll";
const RelativeOrigin = "http://router.invalid";
const QueryOrHash = /[?#]/;

export const RouterContext: ContextKey<RouterState | undefined> = { id: Symbol("reze-router"), defaultValue: undefined };

function parseQuery(search: string): Record<string, string | string[]> {
  const query: Record<string, string | string[]> = Object.create(null);
  if (search === "") return query;
  for (const [key, value] of new URLSearchParams(search)) {
    const existing = query[key];
    if (existing === undefined) query[key] = value;
    else if (typeof existing === "string") query[key] = [existing, value];
    else existing.push(value);
  }
  return query;
}

export function parseLocation(entry: HistoryEntry): Location {
  let path = entry.path;
  let hash = "";
  const hashAt = path.indexOf("#");
  if (hashAt >= 0) {
    hash = path.slice(hashAt);
    path = path.slice(0, hashAt);
  }
  let search = "";
  const searchAt = path.indexOf("?");
  if (searchAt >= 0) {
    search = path.slice(searchAt);
    path = path.slice(0, searchAt);
  }
  return { pathname: path, search, hash, query: parseQuery(search), state: entry.state };
}

/** Router path `href` navigates to when clicked, or `undefined` when the browser handles it (cross-origin, outside the history, malformed). */
export function resolveHref(state: RouterState, href: string): string | undefined {
  let url: URL;
  try {
    url = new URL(href, document.baseURI);
  } catch {
    return undefined;
  }
  return url.origin === location.origin ? state.history.resolve(url) : undefined;
}

/** The pathname of router path `path`, without its search and hash. */
export function pathnameOf(path: string): string {
  const end = path.search(QueryOrHash);
  return end < 0 ? path : path.slice(0, end);
}

/** Loads `route`'s module once; a failure is stored in `loadError` and retried by the next call, never rejected. */
export function ensureLoaded(route: CompiledRoute): Promise<void> {
  const load = route.def.load!;
  return (route.loading ??= Promise.resolve()
    .then(load)
    .then(
      (module) => {
        const config = module.route;
        route.component = module.default ?? route.def.component;
        route.preload = config?.preload ?? route.def.preload;
        route.info = config?.info ?? route.def.info;
        route.loadError = undefined;
        route.isLoaded = true;
      },
      (error: unknown) => {
        route.loadError = error;
        route.loading = undefined;
      },
    ));
}

/** Loads every unloaded route of `match`; resolves once all settled. */
export function loadBranch(match: BranchMatch): Promise<unknown> | undefined {
  let pending: Promise<void>[] | undefined;
  for (const route of match.branch.routes) {
    if (!route.isLoaded) (pending ??= []).push(ensureLoaded(route));
  }
  return pending === undefined ? undefined : Promise.all(pending);
}

function isHashChange(state: RouterState, location: Location): boolean {
  if (state.entry === undefined) return false;
  const committed = untrack(state.location);
  return location.hash !== committed.hash && location.pathname === committed.pathname && location.search === committed.search;
}

export function matchPathname(state: RouterState, pathname: string): BranchMatch | undefined {
  if (pathname !== state.matchedPathname) {
    state.lastMatch = matchBranches(state.branches, pathname);
    state.matchedPathname = pathname;
  }
  return state.lastMatch;
}

export function start(state: RouterState, entry: HistoryEntry, intent: PreloadIntent, scrollMode: ScrollMode): void {
  const generation = ++state.generation;
  const location = parseLocation(entry);
  state.target = entry;
  state.targetLocation = location;
  const isHash = isHashChange(state, location);
  const match = isHash ? undefined : matchPathname(state, location.pathname);
  const finish = (): void => {
    if (generation !== state.generation) return;
    const matches = isHash ? untrack(state.matches) : activate(match, location, intent);
    if (generation === state.generation) commit(state, entry, location, matches, scrollMode);
  };
  const loading = match === undefined ? undefined : loadBranch(match);
  if (loading !== undefined) {
    state.setIsRouting(true);
    state.setPendingKey(pathKey(location.pathname));
    void loading.then(finish);
  } else if (state.renderDepth > 0) {
    queueMicrotask(finish);
  } else {
    finish();
  }
}

function activate(match: BranchMatch | undefined, location: Location, intent: PreloadIntent): ActiveMatch[] {
  const matches: ActiveMatch[] = [];
  if (match === undefined) return matches;
  const { params, path } = match;
  for (const route of match.branch.routes) {
    let data: unknown;
    let error: unknown = route.isLoaded ? undefined : route.loadError;
    if (route.isLoaded && route.preload !== undefined) {
      try {
        data = untrack(() => route.preload!({ params, location, intent }));
      } catch (thrown) {
        error = thrown;
      }
    }
    matches.push({ route, path, params, data, error, info: route.info });
  }
  return matches;
}

function commit(
  state: RouterState,
  entry: HistoryEntry,
  location: Location,
  matches: readonly ActiveMatch[],
  scrollMode: ScrollMode,
): void {
  const isScrollManaged = state.history.scroll;
  if (isScrollManaged && state.entry !== undefined) savePosition(state, state.entry.index);
  state.entry = entry;
  state.setLocation(location);
  state.setMatches(matches);
  state.setIsRouting(false);
  state.setPendingKey(undefined);
  flush();
  if (!isScrollManaged || scrollMode === "none") return;
  if ((scrollMode === "restore" || scrollMode === "initial") && restorePosition(state, entry.index)) return;
  if (location.hash !== "") document.getElementById(decode(location.hash.slice(1)))?.scrollIntoView();
  else if (scrollMode !== "initial") scrollTo(0, 0);
}

function savePosition(state: RouterState, index: number): void {
  const slots = (state.positions ??= new Float64Array(MaxRestorableEntries * 3));
  const at = (index % MaxRestorableEntries) * 3;
  slots[at] = index;
  slots[at + 1] = scrollX;
  slots[at + 2] = scrollY;
}

function restorePosition(state: RouterState, index: number): boolean {
  const slots = state.positions;
  const at = (index % MaxRestorableEntries) * 3;
  if (slots === undefined || slots[at] !== index) return false;
  scrollTo(slots[at + 1]!, slots[at + 2]!);
  return true;
}

/** Reads the positions an earlier document of this tab saved with `persistPositions`. */
export function loadPositions(state: RouterState): void {
  let saved: unknown;
  try {
    saved = JSON.parse(sessionStorage.getItem(PositionsKey) ?? "null");
  } catch {
    return;
  }
  if (Array.isArray(saved) && saved.length === MaxRestorableEntries * 3) state.positions = Float64Array.from(saved as number[]);
}

/** Saves the current entry's position and keeps every position for the next document of this tab. */
export function persistPositions(state: RouterState): void {
  if (state.entry !== undefined) savePosition(state, state.entry.index);
  if (state.positions === undefined) return;
  try {
    sessionStorage.setItem(PositionsKey, JSON.stringify(Array.from(state.positions)));
  } catch {
    return;
  }
}

/** Runs the leave guards unless `to` is the target path; `true` when one prevented the navigation. A throwing guard is reported and does not prevent. */
export function isLeavePrevented(
  state: RouterState,
  to: string | number | null,
  options: NavigateOptions,
  retry: (force?: boolean) => void,
): boolean {
  if (state.leaveListeners.size === 0 || to === state.target?.path) return false;
  let defaultPrevented = false;
  const event: BeforeLeaveEvent = {
    from: untrack(state.location),
    to,
    options,
    get defaultPrevented() {
      return defaultPrevented;
    },
    preventDefault() {
      defaultPrevented = true;
    },
    retry,
  };
  for (const listener of state.leaveListeners) {
    try {
      listener(event);
    } catch (error) {
      reportError(error);
    }
  }
  return defaultPrevented;
}

/** Adds a leave guard, which also guards unloading the document; returns the remover. */
export function addLeaveListener(state: RouterState, listener: (event: BeforeLeaveEvent) => void): () => void {
  const listeners = state.leaveListeners;
  if (listeners.size === 0) addEventListener("beforeunload", state.onUnload);
  listeners.add(listener);
  return () => {
    if (listeners.delete(listener) && listeners.size === 0) removeEventListener("beforeunload", state.onUnload);
  };
}

export function navigate(state: RouterState, to: string | number, options: NavigateOptions = {}, force = false): void {
  if (typeof to === "number") {
    if (force) state.skipNextGuard = true;
    state.history.go(to);
    return;
  }
  const current = state.targetLocation!;
  let url: URL;
  try {
    url = new URL(to, RelativeOrigin + current.pathname + current.search);
  } catch {
    return;
  }
  let path: string | undefined;
  if (url.origin === RelativeOrigin) {
    // `paths` builders emit the served base; strip it back to the router path the branches match.
    path = state.history.resolve(url) ?? url.pathname + url.search + url.hash;
  } else {
    path = resolveHref(state, to);
    if (path === undefined) {
      if (url.protocol !== "javascript:") location.assign(to);
      return;
    }
  }
  if (!force && isLeavePrevented(state, path, options, (retryForce = true) => navigate(state, to, options, retryForce))) return;
  if (options.replace === true || path === state.target?.path) state.history.replace(path, options.state);
  else state.history.push(path, options.state);
  start(state, state.history.get(), "navigate", options.scroll === false ? "none" : "top");
}

export function onPop(state: RouterState, entry: HistoryEntry): void {
  if (state.ignorePop) {
    state.ignorePop = false;
    return;
  }
  if (state.skipNextGuard) {
    state.skipNextGuard = false;
  } else {
    const delta = entry.index - (state.target?.index ?? 0);
    const retry = (force = true): void => {
      if (force) state.skipNextGuard = true;
      state.history.go(delta);
    };
    if (delta !== 0 && isLeavePrevented(state, delta, {}, retry)) {
      state.ignorePop = true;
      state.history.go(-delta);
      return;
    }
  }
  start(state, entry, "navigate", "restore");
}
