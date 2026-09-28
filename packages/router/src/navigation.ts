import { flush, untrack, type ContextKey, type Getter, type Owner, type Setter } from "@rezejs/signals";

import type { HistoryEntry, RouterHistory } from "./history";
import { matchBranches, pathKey, type Branch, type BranchMatch, type CompiledRoute } from "./match";
import type { BeforeLeaveEvent, Location, NavigateOptions, Params, PreloadIntent } from "./types";

export interface ActiveMatch {
  readonly route: CompiledRoute;
  readonly path: string;
  readonly params: Params;
  readonly data: unknown;
  readonly error: unknown;
  readonly info: Readonly<Record<string, unknown>> | undefined;
}

type ScrollMode = "top" | "restore" | "none";

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
  entry: HistoryEntry | undefined;
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
  generation: number;
  readonly leaveListeners: Set<(event: BeforeLeaveEvent) => void>;
  ignorePop: boolean;
  skipNextGuard: boolean;
  /** `[index, scrollX, scrollY]` per slot `index % MaxRestorableEntries`, more than browsers keep in session history (50 in Chrome and Firefox, 100 in WebKit); allocated on first save. */
  positions: Float64Array | undefined;
}

const MaxRestorableEntries = 128;

export const RouterContext: ContextKey<RouterState | undefined> = { id: Symbol("reze-router"), defaultValue: undefined };

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
  return { pathname: path, search, hash, query: Object.fromEntries(new URLSearchParams(search)), state: entry.state };
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

export function start(state: RouterState, entry: HistoryEntry, intent: PreloadIntent, scrollMode: ScrollMode): void {
  const generation = ++state.generation;
  const location = parseLocation(entry);
  const match = matchBranches(state.branches, location.pathname);
  const loading = match === undefined ? undefined : loadBranch(match);
  if (loading === undefined) {
    commit(state, entry, location, match, intent, scrollMode);
    return;
  }
  state.setIsRouting(true);
  state.setPendingKey(pathKey(location.pathname));
  void loading.then(() => {
    if (generation === state.generation) commit(state, entry, location, match, intent, scrollMode);
  });
}

function commit(
  state: RouterState,
  entry: HistoryEntry,
  location: Location,
  match: BranchMatch | undefined,
  intent: PreloadIntent,
  scrollMode: ScrollMode,
): void {
  const matches: ActiveMatch[] = [];
  if (match !== undefined) {
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
  }
  const isScrollManaged = state.history.scroll;
  if (isScrollManaged && state.entry !== undefined) savePosition(state, state.entry.index);
  state.entry = entry;
  state.setLocation(location);
  state.setMatches(matches);
  state.setIsRouting(false);
  state.setPendingKey(undefined);
  flush();
  if (!isScrollManaged) return;
  if (scrollMode === "restore") {
    restorePosition(state, entry.index);
  } else if (scrollMode === "top") {
    if (location.hash === "") scrollTo(0, 0);
    else document.getElementById(decodeURIComponent(location.hash.slice(1)))?.scrollIntoView();
  }
}

function savePosition(state: RouterState, index: number): void {
  const slots = (state.positions ??= new Float64Array(MaxRestorableEntries * 3));
  const at = (index % MaxRestorableEntries) * 3;
  slots[at] = index;
  slots[at + 1] = scrollX;
  slots[at + 2] = scrollY;
}

function restorePosition(state: RouterState, index: number): void {
  const slots = state.positions;
  const at = (index % MaxRestorableEntries) * 3;
  if (slots !== undefined && slots[at] === index) scrollTo(slots[at + 1]!, slots[at + 2]!);
  else scrollTo(0, 0);
}

/** Runs the leave guards unless `to` is the current path; `true` when one prevented the navigation. */
export function isLeavePrevented(
  state: RouterState,
  to: string | number,
  options: NavigateOptions,
  retry: (force?: boolean) => void,
): boolean {
  if (state.leaveListeners.size === 0 || to === state.entry?.path) return false;
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
  for (const listener of state.leaveListeners) listener(event);
  return defaultPrevented;
}

export function navigate(state: RouterState, to: string | number, options: NavigateOptions = {}, force = false): void {
  if (typeof to === "number") {
    if (force) state.skipNextGuard = true;
    state.history.go(to);
    return;
  }
  const current = untrack(state.location);
  const url = new URL(to, "http://r" + current.pathname + current.search);
  const path = url.pathname + url.search + url.hash;
  if (!force && isLeavePrevented(state, path, options, (retryForce = true) => navigate(state, to, options, retryForce))) return;
  if (options.replace === true || path === state.entry?.path) state.history.replace(path, options.state);
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
    const delta = entry.index - (state.entry?.index ?? 0);
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
