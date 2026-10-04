import { flush, signal, untrack, type ContextKey, type Getter, type Owner, type Setter } from "@rezejs/signals";

import type { HistoryEntry, RouterHistory } from "./history";
import { decode, matchBranches, pathKey, type Branch, type BranchMatch, type CompiledRoute } from "./match";
import type { Awaitable, BeforeLeaveEvent, Location, NavigateOptions, PageMetadata, Params, PreloadIntent, RouteRedirect } from "./types";

export interface ActiveMatch {
  readonly route: CompiledRoute;
  readonly path: string;
  readonly params: Params;
  readonly data: unknown;
  /** False when the route has no preload or its preload failed; true even when the settled data is `undefined`. */
  readonly hasData: boolean;
  readonly error: unknown;
  /** The route's own resolved metadata, before the root-to-leaf merge. */
  readonly meta: PageMetadata;
  readonly info: Readonly<Record<string, unknown>> | undefined;
}

/** Where the router runs: page browser, SSG page preparation, or hydration preparation. */
export type RouterEnv = "browser" | "html" | "hydrate";

/** Staged-session hook the browser hydration boot passes through; cleared after the initial commit. */
export interface RouterCommitHost {
  deferCommit(fn: () => void | (() => void)): void;
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
  /** Browser page, SSG page preparation, or hydration preparation; hydration flips to browser on its initial commit. */
  env: RouterEnv;
  /** Set during hydration preparation; framework writes and listeners defer through it until the initial commit clears it. */
  commitHost: RouterCommitHost | undefined;
  /** Imperative navigation captured during SSG preparation instead of touching history. */
  redirectCaptured: { to: string; replace: boolean } | undefined;
  /** Template metadata the merged page metadata restores absent fields to; from config or captured on first apply. */
  headBaseline: PageMetadata | undefined;
  /** Hover-warming depth: while nonzero, preloads run but navigations are ignored and metadata/redirects never run. */
  warming: number;
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
  /** `pathKey` of the pathname a navigation is loading route modules or awaiting data for. */
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
export const NoMatches: readonly ActiveMatch[] = [];

export interface RouterStateInit {
  readonly history: RouterHistory;
  readonly branches: readonly Branch[];
  readonly env: RouterEnv;
  readonly headBaseline?: PageMetadata;
  readonly commitHost?: RouterCommitHost;
}

/** Builds a `RouterState` for any environment; the caller sets `owner` inside the router component setup. */
export function initRouterState(init: RouterStateInit): RouterState {
  const [location, setLocation] = signal(parseLocation(init.history.get()));
  const [matches, setMatches] = signal(NoMatches);
  const [isRouting, setIsRouting] = signal(true);
  const [pendingKey, setPendingKey] = signal<string | undefined>(undefined);
  const state: RouterState = {
    history: init.history,
    branches: init.branches,
    env: init.env,
    commitHost: init.commitHost,
    redirectCaptured: undefined,
    headBaseline: init.headBaseline,
    warming: 0,
    owner: undefined,
    entry: undefined,
    target: undefined,
    targetLocation: undefined,
    location,
    setLocation,
    matches,
    setMatches,
    isRouting,
    setIsRouting,
    pendingKey,
    setPendingKey,
    links: undefined,
    renderDepth: 0,
    matchedPathname: undefined,
    lastMatch: undefined,
    generation: 0,
    leaveListeners: new Set(),
    onUnload: (event) => {
      if (!isLeavePrevented(state, null, {}, () => {})) return;
      event.preventDefault();
      event.returnValue = true;
    },
    ignorePop: false,
    skipNextGuard: false,
    positions: undefined,
  };
  return state;
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
/** A promise-like of any shape, not just `instanceof Promise`. */
export function isThenable<T>(value: T | PromiseLike<T>): value is PromiseLike<T> {
  return (
    (typeof value === "object" && value !== null) ||
    typeof value === "function"
  ) && typeof (value as PromiseLike<T>).then === "function";
}

/** `resolveHref` outside the page: parses against a dummy origin and strips the router base, never touching `document`. */
function resolveHrefNoDom(state: RouterState, href: string): string | undefined {
  let url: URL;
  try {
    url = new URL(href, RelativeOrigin + "/");
  } catch {
    return undefined;
  }
  if (url.origin !== RelativeOrigin) return undefined;
  return state.history.resolve(url);
}

/**
 * Router path `href` navigates to when clicked, or `undefined` when the browser handles it. Uses the live document on
 * page browser routers and the base-aware history without `document` during SSG preparation.
 */
export function resolveHrefSafe(state: RouterState, href: string): string | undefined {
  return state.env === "browser" ? resolveHref(state, href) : resolveHrefNoDom(state, href);
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
        route.meta = config?.meta ?? route.def.meta;
        route.redirect = config?.redirect ?? route.def.redirect;
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

/** The settled result of one navigation: rendered matches, one redirect step, or abandonment by a newer navigation. */
export type SettleOutcome =
  | { kind: "render"; matches: ActiveMatch[]; metadata: PageMetadata | undefined }
  | { kind: "redirect"; to: string; replace: boolean }
  | { kind: "aborted" };

export function start(state: RouterState, entry: HistoryEntry, intent: PreloadIntent, scrollMode: ScrollMode): void {
  const generation = ++state.generation;
  const location = parseLocation(entry);
  state.target = entry;
  state.targetLocation = location;
  if (isHashChange(state, location)) {
    const matches = untrack(state.matches);
    const apply = (): void => {
      if (generation === state.generation) commit(state, entry, location, [...matches], undefined, scrollMode);
    };
    if (state.renderDepth > 0) queueMicrotask(apply);
    else apply();
    return;
  }
  const match = matchPathname(state, location.pathname);
  const begin = (): void => {
    if (generation !== state.generation) return;
    const outcome: SettleOutcome | Promise<SettleOutcome> =
      match === undefined ? { kind: "render", matches: [], metadata: {} } : settleMatch(state, generation, match, location, intent);
    if (isThenable(outcome)) {
      state.setIsRouting(true);
      state.setPendingKey(pathKey(location.pathname));
      void outcome.then(
        (resolved) => finishSettled(state, generation, entry, location, resolved, scrollMode),
        (error) => {
          reportError(error);
        },
      );
    } else if (state.renderDepth > 0) {
      queueMicrotask(() => finishSettled(state, generation, entry, location, outcome, scrollMode));
    } else {
      finishSettled(state, generation, entry, location, outcome, scrollMode);
    }
  };
  const loading = match === undefined ? undefined : loadBranch(match);
  if (loading !== undefined) {
    state.setIsRouting(true);
    state.setPendingKey(pathKey(location.pathname));
    void loading.then(begin);
  } else {
    begin();
  }
}

function finishSettled(
  state: RouterState,
  generation: number,
  entry: HistoryEntry,
  location: Location,
  outcome: SettleOutcome,
  scrollMode: ScrollMode,
): void {
  if (generation !== state.generation) return;
  if (outcome.kind === "aborted") return;
  if (outcome.kind === "redirect") {
    if (state.env === "html") {
      state.redirectCaptured = { to: outcome.to, replace: outcome.replace };
      return;
    }
    navigate(state, outcome.to, { replace: outcome.replace });
    return;
  }
  commit(state, entry, location, outcome.matches, outcome.metadata, scrollMode);
}

/** Runs one entry's match to completion without committing; SSG preparation drives this directly. */
export function settleEntry(state: RouterState, entry: HistoryEntry, intent: PreloadIntent): Promise<SettleOutcome> {
  const generation = ++state.generation;
  const location = parseLocation(entry);
  state.target = entry;
  state.targetLocation = location;
  const match = matchPathname(state, location.pathname);
  if (match === undefined) return Promise.resolve({ kind: "render", matches: [], metadata: {} });
  const loading = loadBranch(match);
  const settle = (): Promise<SettleOutcome> | SettleOutcome => {
    if (generation !== state.generation) return { kind: "aborted" };
    return settleMatch(state, generation, match, location, intent);
  };
  return loading === undefined ? Promise.resolve().then(settle) : loading.then(settle);
}

function checkpoint(state: RouterState, generation: number): SettleOutcome | undefined {
  const captured = state.redirectCaptured;
  if (captured !== undefined) return { kind: "redirect", to: captured.to, replace: captured.replace };
  if (generation !== state.generation) return { kind: "aborted" };
  return undefined;
}

function settleMatch(
  state: RouterState,
  generation: number,
  match: BranchMatch,
  location: Location,
  intent: PreloadIntent,
): SettleOutcome | Promise<SettleOutcome> {
  const routes = match.branch.routes;
  for (const route of routes) {
    const redirect = route.redirect;
    if (redirect !== undefined && typeof redirect !== "function") {
      return { kind: "redirect", to: redirect.to, replace: redirect.replace ?? true };
    }
  }
  return settleRoute(state, generation, routes, match.params, match.path, location, intent, 0, []);
}

function settleRoute(
  state: RouterState,
  generation: number,
  routes: readonly CompiledRoute[],
  params: Params,
  path: string,
  location: Location,
  intent: PreloadIntent,
  index: number,
  matches: ActiveMatch[],
): SettleOutcome | Promise<SettleOutcome> {
  if (index >= routes.length) {
    const metadata: PageMetadata = {};
    for (const settled of matches) {
      if (settled.meta.title !== undefined) metadata.title = settled.meta.title;
      if (settled.meta.description !== undefined) metadata.description = settled.meta.description;
      if (settled.meta.canonical !== undefined) metadata.canonical = settled.meta.canonical;
      if (settled.meta.robots !== undefined) metadata.robots = settled.meta.robots;
    }
    return { kind: "render", matches, metadata };
  }
  const stopped = checkpoint(state, generation);
  if (stopped !== undefined) return stopped;
  const route = routes[index]!;
  if (!route.isLoaded) {
    matches.push({ route, path, params, data: undefined, hasData: false, error: route.loadError, meta: {}, info: route.info });
    return settleRoute(state, generation, routes, params, path, location, intent, index + 1, matches);
  }
  if (route.preload === undefined) {
    return settleData(state, generation, routes, params, path, location, intent, index, matches, undefined, false, undefined);
  }
  let produced: unknown;
  try {
    produced = untrack(() => route.preload!({ params, location, intent }));
  } catch (thrown) {
    return settleData(state, generation, routes, params, path, location, intent, index, matches, undefined, false, thrown);
  }
  if (isThenable(produced)) {
    return Promise.resolve(produced).then(
      (value) => {
        const early = checkpoint(state, generation);
        if (early !== undefined) return early;
        return settleData(state, generation, routes, params, path, location, intent, index, matches, value, true, undefined);
      },
      (thrown) => {
        const early = checkpoint(state, generation);
        if (early !== undefined) return early;
        return settleData(state, generation, routes, params, path, location, intent, index, matches, undefined, false, thrown);
      },
    );
  }
  return settleData(state, generation, routes, params, path, location, intent, index, matches, produced, true, undefined);
}

function settleData(
  state: RouterState,
  generation: number,
  routes: readonly CompiledRoute[],
  params: Params,
  path: string,
  location: Location,
  intent: PreloadIntent,
  index: number,
  matches: ActiveMatch[],
  data: unknown,
  hasData: boolean,
  error: unknown,
): SettleOutcome | Promise<SettleOutcome> {
  const route = routes[index]!;
  const push = (ownMeta: PageMetadata, failure: unknown): SettleOutcome | Promise<SettleOutcome> => {
    matches.push({ route, path, params, data, hasData: failure === undefined ? hasData : false, error: failure, meta: ownMeta, info: route.info });
    return settleRoute(state, generation, routes, params, path, location, intent, index + 1, matches);
  };
  const afterMeta = (ownMeta: PageMetadata): SettleOutcome | Promise<SettleOutcome> => {
    const redirect = route.redirect;
    if (error !== undefined || redirect === undefined || typeof redirect !== "function") return push(ownMeta, error);
    const args = { params, location, intent, data };
    let decided: Awaitable<RouteRedirect | undefined>;
    try {
      decided = untrack(() => redirect(args));
    } catch (thrown) {
      return push(ownMeta, thrown);
    }
    if (isThenable(decided)) {
      return Promise.resolve(decided).then(
        (value) => {
          const early = checkpoint(state, generation);
          if (early !== undefined) return early;
          if (value === undefined || value === null) return push(ownMeta, error);
          return { kind: "redirect", to: value.to, replace: value.replace ?? true };
        },
        (thrown) => {
          const early = checkpoint(state, generation);
          if (early !== undefined) return early;
          return push(ownMeta, thrown);
        },
      );
    }
    if (decided === undefined || decided === null) return push(ownMeta, error);
    return { kind: "redirect", to: decided.to, replace: decided.replace ?? true };
  };
  const meta = route.meta;
  if (error !== undefined || meta === undefined) return afterMeta({});
  if (typeof meta !== "function") return afterMeta(meta);
  const resolvedArgs = { params, location, intent, data };
  let produced: Awaitable<PageMetadata>;
  try {
    produced = untrack(() => meta(resolvedArgs));
  } catch (thrown) {
    return push({}, thrown);
  }
  if (isThenable(produced)) {
    return Promise.resolve(produced).then(
      (value) => {
        const early = checkpoint(state, generation);
        if (early !== undefined) return early;
        return afterMeta(value ?? {});
      },
      (thrown) => {
        const early = checkpoint(state, generation);
        if (early !== undefined) return early;
        return push({}, thrown);
      },
    );
  }
  return afterMeta(produced ?? {});
}

/** Commits settled matches; SSG and hydration preparation share this with `start`. `metadata: undefined` skips head writes (hash changes, hydrated initial commit). */
export function commit(
  state: RouterState,
  entry: HistoryEntry,
  location: Location,
  matches: readonly ActiveMatch[],
  metadata: PageMetadata | undefined,
  scrollMode: ScrollMode,
): void {
  const isScrollManaged = state.env === "browser" && state.history.scroll;
  if (isScrollManaged && state.entry !== undefined) savePosition(state, state.entry.index);
  state.entry = entry;
  state.setLocation(location);
  state.setMatches(matches);
  state.setIsRouting(false);
  state.setPendingKey(undefined);
  const host = state.commitHost;
  if (host !== undefined) host.deferCommit(() => {
    state.commitHost = undefined;
    state.env = "browser";
  });
  if (metadata !== undefined) {
    if (host !== undefined) host.deferCommit(() => applyMetadata(state, metadata));
    else applyMetadata(state, metadata);
  }
  flush();
  if (!isScrollManaged || scrollMode === "none") return;
  if ((scrollMode === "restore" || scrollMode === "initial") && restorePosition(state, entry.index)) return;
  if (location.hash !== "") document.getElementById(decode(location.hash.slice(1)))?.scrollIntoView();
  else if (scrollMode !== "initial") scrollTo(0, 0);
}

function readHeadMetadata(): PageMetadata {
  const description = document.querySelector('meta[name="description"]')?.getAttribute("content") ?? undefined;
  const canonical = document.querySelector('link[rel="canonical"]')?.getAttribute("href") ?? undefined;
  const robots = document.querySelector('meta[name="robots"]')?.getAttribute("content") ?? undefined;
  return { title: document.title || undefined, description, canonical, robots };
}

function writeHeadText(kind: "description" | "robots", value: string | undefined): void {
  const selector = `meta[name="${kind}"]`;
  if (value === undefined) {
    document.querySelector(selector)?.remove();
    return;
  }
  const existing = document.querySelector(selector);
  if (existing !== null) {
    existing.setAttribute("content", value);
    return;
  }
  const tag = document.createElement("meta");
  tag.setAttribute("name", kind);
  tag.setAttribute("content", value);
  document.head.append(tag);
}

function writeHeadCanonical(value: string | undefined): void {
  if (value === undefined) {
    document.querySelector('link[rel="canonical"]')?.remove();
    return;
  }
  const existing = document.querySelector('link[rel="canonical"]');
  if (existing !== null) {
    existing.setAttribute("href", value);
    return;
  }
  const tag = document.createElement("link");
  tag.setAttribute("rel", "canonical");
  tag.setAttribute("href", value);
  document.head.append(tag);
}

function applyMetadata(state: RouterState, metadata: PageMetadata): void {
  if (state.env !== "browser" || typeof document === "undefined") return;
  const baseline = (state.headBaseline ??= readHeadMetadata());
  const title = metadata.title ?? baseline.title;
  if (title !== undefined) document.title = title;
  writeHeadText("description", metadata.description ?? baseline.description);
  writeHeadCanonical(metadata.canonical ?? baseline.canonical);
  writeHeadText("robots", metadata.robots ?? baseline.robots);
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
  if (typeof sessionStorage === "undefined") return;
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
  if (typeof sessionStorage === "undefined") return;
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
  const page = state.env !== "html" && typeof addEventListener === "function";
  if (listeners.size === 0 && page) addEventListener("beforeunload", state.onUnload);
  listeners.add(listener);
  return () => {
    if (listeners.delete(listener) && listeners.size === 0 && page) removeEventListener("beforeunload", state.onUnload);
  };
}

export function navigate(state: RouterState, to: string | number, options: NavigateOptions = {}, force = false): void {
  if (typeof to === "number") {
    if (state.env !== "browser") return;
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
    path = state.env === "browser" ? resolveHref(state, to) : resolveHrefNoDom(state, to);
    if (path === undefined) {
      if (state.env === "browser" && url.protocol !== "javascript:") location.assign(to);
      return;
    }
  }
  if (!force && isLeavePrevented(state, path, options, (retryForce = true) => navigate(state, to, options, retryForce))) return;
  // Hover warming runs preloads only: a navigation it triggers is ignored, never committed or captured.
  if (state.warming > 0) return;
  // SSG preparation captures imperative navigation as a redirect instead of touching history.
  if (state.env === "html") {
    state.redirectCaptured = { to: path, replace: options.replace ?? false };
    state.generation++;
    return;
  }
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
