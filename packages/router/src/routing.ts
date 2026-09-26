// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { createComponent, createContext, startTransition } from "@rezejs/dom";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import {
  computed,
  getOwner,
  runWithOwner,
  signal,
  untrack,
  useContext,
  type Getter,
  type Owner,
  type Setter,
} from "@rezejs/signals";

import {
  getRouteMatches,
  resolveLazySubtree,
  trackLazySubtrees,
  unresolvedLazyMatches,
} from "./matching";
import { HREF } from "./paths";
import type { QueryCache } from "./query";
import type {
  BeforeLeaveSlot,
  Branch,
  Intent,
  LazyBoundary,
  Location,
  LocationChange,
  MatchFilters,
  NavigateOptions,
  Navigator,
  Params,
  PathMatch,
  RouteContext,
  RouteDefinition,
  RouteMatch,
  RouteParams,
  RoutePreloadFunc,
  RouteSectionProps,
  RouterUtils,
  SearchParams,
  SetSearchParams,
  Submission,
  TypedPath,
  TypedSearchPath,
} from "./types";
import {
  comparablePath,
  createMatcher,
  createMemoObject,
  expandOptionals,
  extractSearchParams,
  isSameLocationChange,
  isThenable,
  mergeParams,
  mergeSearchString,
  mockBase,
  resolvePath,
  validateSearch,
} from "./utils";

const MaxRedirects = 100;

/**
 * The location source the router core reads and writes: `signal` holds the committed location,
 * `commit` writes a programmatic navigation to the history, `listen` reports locations the
 * history reached on its own (back/forward).
 */
export interface RouterIntegration {
  signal: [get: Getter<LocationChange>, set: (next: LocationChange) => void];
  commit?(next: LocationChange): void;
  listen?(notify: (next: LocationChange) => void): void;
  utils?: Partial<RouterUtils>;
}

type RouteKey = RouteDefinition | LazyBoundary;

type NavigationTarget = string | TypedPath | ((headed: LocationChange) => string);

export interface RouterState {
  base: RouteContext;
  location: Location;
  params: Params;
  wrapParams(getParams: () => Params): Params;
  navigatorFactory(route?: RouteContext): Navigator;
  isRouting: Getter<boolean>;
  intent(): Intent | undefined;
  pendingTarget: Getter<LocationChange | undefined>;
  matches: Getter<RouteMatch[]>;
  renderPath(path: string): string;
  parsePath(path: string): string;
  beforeLeave: BeforeLeaveSlot;
  preloadRoute(url: URL, preloadData?: boolean): void;
  takePreloaded(key: RouteKey): { data: unknown } | undefined;
  singleFlight: boolean;
  submissions: [Getter<Submission<unknown, unknown>[]>, Setter<Submission<unknown, unknown>[]>];
  queryCache: QueryCache | undefined;
}

export const RouterContext = /* @__PURE__ */ createContext<RouterState>();
export const RouteContextKey = /* @__PURE__ */ createContext<RouteContext>();

export function useRouter(): RouterState {
  const router = useContext(RouterContext);
  if (router === undefined) {
    throw new Error("router primitives can be only used inside a Router");
  }
  return router;
}

export function useRoute(): RouteContext {
  return useContext(RouteContextKey) ?? useRouter().base;
}

let preloadIntent: Intent | undefined;
let isInPreloadFn = false;

/** The intent of the navigation or preload running now, as `query` records it. */
export function getIntent(): Intent | undefined {
  return preloadIntent ?? (getOwner() ? useContext(RouterContext)?.intent() : undefined);
}

export function getInPreloadFn(): boolean {
  return isInPreloadFn;
}

export function runPreload<T>(
  preload: RoutePreloadFunc<T>,
  args: Parameters<RoutePreloadFunc<T>>[0],
): T {
  const wasInPreloadFn = isInPreloadFn;
  isInPreloadFn = true;
  try {
    return untrack(() => preload(args));
  } finally {
    isInPreloadFn = wasInPreloadFn;
  }
}

/** Runs `fn` with `query` recording `intent`: preload passes, the server collector, link preloads. */
export function runWithIntent<T>(intent: Intent, fn: () => T): T {
  const previous = preloadIntent;
  preloadIntent = intent;
  try {
    return fn();
  } finally {
    preloadIntent = previous;
  }
}

function staticLocation(url: URL): Location {
  return {
    pathname: url.pathname,
    search: url.search,
    hash: url.hash,
    query: extractSearchParams(url),
    state: null,
    key: "",
  };
}

function toURL(path: string): URL {
  return new URL(path[0] === "/" ? mockBase + path : path, mockBase);
}

function createLocation(
  path: Getter<string>,
  state: Getter<unknown>,
  queryWrapper?: RouterUtils["queryWrapper"],
): Location {
  const url = computed<URL>((previous) => {
    const next = path();
    try {
      const parsed = toURL(next);
      return previous !== undefined && previous.href === parsed.href ? previous : parsed;
    } catch {
      if (process.env.NODE_ENV !== "production") console.error(`Invalid path ${next}`);
      return previous ?? new URL(mockBase);
    }
  });
  const pathname = computed(() => url().pathname);
  const search = computed(() => url().search);
  const hash = computed(() => url().hash);
  const query = computed(() => extractSearchParams(url()));
  return {
    get pathname() {
      return pathname();
    },
    get search() {
      return search();
    },
    get hash() {
      return hash();
    },
    get state() {
      return state() as Location["state"];
    },
    get key() {
      return "";
    },
    query: queryWrapper ? queryWrapper(query) : createMemoObject(query),
  };
}

export interface RouterOptions {
  base?: string;
  singleFlight?: boolean;
  transformUrl?: (url: string) => string;
  isServer: boolean;
}

export function createRouterContext(
  integration: RouterIntegration,
  branches: Getter<Branch[]>,
  options: RouterOptions,
): RouterState {
  const {
    signal: [source, setSource],
    utils = {},
  } = integration;
  const parsePath = utils.parsePath ?? ((path: string) => path);
  const renderPath = utils.renderPath ?? ((path: string) => path);
  const beforeLeave = utils.beforeLeave ?? {};
  const basePath = resolvePath("", options.base ?? "");
  if (basePath === undefined) throw new Error(`${options.base} is not a valid base path`);
  const transformUrl = options.transformUrl ?? ((path: string) => path);
  const location = createLocation(
    () => source().value,
    () => source().state,
    utils.queryWrapper,
  );
  const matches = computed(() => {
    const found = getRouteMatches(branches(), transformUrl(location.pathname));
    for (const boundary of unresolvedLazyMatches(found)) {
      if (boundary.error === undefined) resolveLazySubtree(boundary).catch(() => {});
    }
    return found;
  });
  const isResolvingLazy = computed(() => {
    trackLazySubtrees();
    return unresolvedLazyMatches(matches()).some((boundary) => boundary.error === undefined);
  });
  const [navigation, setNavigation] = signal<
    { target: LocationChange; intent: Intent } | undefined
  >(undefined);
  const isRouting = computed(() => navigation() !== undefined || isResolvingLazy());
  const pendingTarget = computed(() => {
    const current = navigation();
    return current?.intent === "navigate" ? current.target : undefined;
  });
  const wrapParams = utils.paramsWrapper
    ? (getParams: () => Params) => utils.paramsWrapper!(getParams, branches)
    : (getParams: () => Params) => createMemoObject(computed(getParams));
  const params = wrapParams(() => mergeParams(matches()));
  const baseRoute: RouteContext = {
    pattern: basePath,
    params,
    path: () => basePath,
    outlet: () => null,
    resolvePath: (to) => resolvePath(basePath, to),
  };
  let contextOwner: Owner | undefined;
  let committingIntent: Intent | undefined;
  let preloaded: Map<RouteKey, unknown> | undefined;
  let headed: LocationChange | undefined;
  let hops = 0;
  let navigationId = 0;
  let submissions: RouterState["submissions"] | undefined;

  const router: RouterState = {
    base: baseRoute,
    location,
    params,
    wrapParams,
    navigatorFactory,
    isRouting,
    intent: () => committingIntent,
    pendingTarget,
    matches,
    renderPath,
    parsePath,
    beforeLeave,
    preloadRoute,
    takePreloaded(key) {
      if (!preloaded?.has(key)) return undefined;
      const data = preloaded.get(key);
      preloaded.delete(key);
      return { data };
    },
    singleFlight: options.singleFlight ?? true,
    get submissions() {
      return (submissions ??= signal<Submission<unknown, unknown>[]>([]));
    },
    queryCache: options.isServer ? new Map() : undefined,
  };
  contextOwner = getOwner();

  if (basePath && !untrack(source).value) {
    setSource({ value: basePath, replace: true, scroll: false });
  }
  integration.listen?.((next) => startNavigation(next, "native"));
  return router;

  function startNavigation(next: LocationChange, intent: "navigate" | "native"): void {
    const id = ++navigationId;
    headed = next;
    setNavigation({ target: next, intent });
    const commit = (data: Map<RouteKey, unknown>): void => {
      if (id !== navigationId) return;
      headed = undefined;
      hops = 0;
      committingIntent = intent;
      preloaded = data;
      let transition: Promise<void>;
      try {
        transition = startTransition(() => {
          setSource(next);
          if (intent === "navigate") integration.commit?.(next);
        });
      } finally {
        committingIntent = undefined;
      }
      void transition.then(() => {
        if (id !== navigationId) return;
        preloaded = undefined;
        setNavigation(undefined);
      });
    };
    const prepared = prepareNavigation(toURL(next.value), intent);
    if (prepared instanceof Promise) void prepared.then(commit);
    else commit(prepared);
  }

  function prepareNavigation(
    url: URL,
    intent: Intent,
  ): Map<RouteKey, unknown> | Promise<Map<RouteKey, unknown>> {
    const next = getRouteMatches(untrack(branches), transformUrl(url.pathname));
    const lazy = unresolvedLazyMatches(next);
    if (lazy.length) {
      return Promise.all(lazy.map(resolveLazySubtree)).then(
        () => prepareNavigation(url, intent),
        () => new Map(),
      );
    }
    const current = untrack(matches);
    const data = new Map<RouteKey, unknown>();
    const waits: Promise<unknown>[] = [];
    const nextLocation = staticLocation(url);
    const nextParams = mergeParams(next);
    const previousIntent = preloadIntent;
    preloadIntent = intent;
    try {
      next.forEach(({ route }, level) => {
        const load = (route.component as { preload?: () => unknown } | undefined)?.preload;
        if (typeof load === "function") waits.push(Promise.resolve(load()));
        if (current[level]?.route.key === route.key || route.preload === undefined) return;
        const value = runWithOwner(contextOwner, () =>
          runPreload(route.preload!, { params: nextParams, location: nextLocation, intent }),
        );
        data.set(route.key, value);
        if (isThenable(value)) waits.push(Promise.resolve(value));
      });
    } finally {
      preloadIntent = previousIntent;
    }
    if (!waits.length) return data;
    return Promise.allSettled(waits).then(() => data);
  }

  function navigateTo(
    route: RouteContext,
    to: NavigationTarget | number,
    navigateOptions?: Partial<NavigateOptions>,
  ): void {
    untrack(() => {
      if (typeof to === "number") {
        if (!to) return;
        if (utils.go) utils.go(to);
        else if (process.env.NODE_ENV !== "production") {
          console.warn("Router integration does not support relative routing");
        }
        return;
      }
      const {
        replace,
        resolve,
        scroll,
        state: nextState,
      } = { replace: false, resolve: true, scroll: true, ...navigateOptions };
      const resolveTarget = (target: string): string | undefined => {
        if (target[0] === "#") target = parsePath(target);
        if (!resolve) {
          return resolvePath(((!target || target[0] === "?") && location.pathname) || "", target);
        }
        if (target[0] === "/") return route.resolvePath(target);
        const url = new URL(target, mockBase + location.pathname + location.search + location.hash);
        return url.origin === mockBase ? url.pathname + url.search + url.hash : undefined;
      };
      const current = headed ?? source();
      const href = typeof to === "string" ? undefined : (to as { [HREF]?: string })[HREF];
      const raw =
        typeof to === "string"
          ? to
          : href !== undefined
            ? href
            : typeof to === "function"
              ? (to as (headed: LocationChange) => string)(current)
              : String(to);
      const value = resolveTarget(raw);
      if (value === undefined) throw new Error(`Path '${raw}' is not a routable path`);
      const isHop = headed !== undefined;
      if (isHop && ++hops >= MaxRedirects) throw new Error("Too many redirects");
      const next: LocationChange = {
        value,
        state: nextState,
        replace: isHop ? headed!.replace : replace,
        scroll: isHop ? headed!.scroll : scroll,
      };
      if (isSameLocationChange(next, current)) return;
      if (options.isServer) {
        setSource(next);
        return;
      }
      if (beforeLeave.current && !beforeLeave.current.confirm(value, navigateOptions)) return;
      startNavigation(next, "navigate");
    });
  }

  function navigatorFactory(route?: RouteContext): Navigator {
    const from = route ?? (getOwner() ? useContext(RouteContextKey) : undefined) ?? baseRoute;
    return ((to: NavigationTarget | number, navigateOptions?: Partial<NavigateOptions>) =>
      navigateTo(from, to, navigateOptions)) as Navigator;
  }

  function preloadRoute(url: URL, preloadData?: boolean): void {
    const next = getRouteMatches(untrack(branches), transformUrl(url.pathname));
    const lazy = unresolvedLazyMatches(next);
    if (lazy.length) {
      Promise.all(lazy.map(resolveLazySubtree)).then(
        () => preloadRoute(url, preloadData),
        () => {},
      );
    }
    const current = untrack(matches);
    const nextLocation = staticLocation(url);
    const previousIntent = preloadIntent;
    preloadIntent = "preload";
    try {
      next.forEach(({ route, params: routeParams }, level) => {
        (route.component as { preload?: () => unknown } | undefined)?.preload?.();
        const now = current[level];
        const isUnchanged =
          now !== undefined &&
          now.route.key === route.key &&
          sameParams(now.params, routeParams) &&
          url.search === untrack(() => location.search);
        if (!preloadData || isUnchanged || route.preload === undefined) return;
        const value = runWithOwner(contextOwner, () =>
          runPreload(route.preload!, {
            params: routeParams,
            location: nextLocation,
            intent: "preload",
          }),
        );
        if (isThenable(value)) Promise.resolve(value).catch(() => {});
      });
    } finally {
      preloadIntent = previousIntent;
    }
  }
}

function sameParams(a: Params, b: Params): boolean {
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((key) => a[key] === b[key]);
}

/** Renders the route chain from `depth` down; each level is recreated only when its route changes. */
export function renderRoutes(
  router: RouterState,
  depth: number,
  parent: RouteContext,
): JSX.Element {
  const key = computed<RouteKey | undefined>(() => router.matches()[depth]?.route.key);
  return computed(() => {
    const routeKey = key();
    if (routeKey === undefined) return undefined;
    return untrack(() => {
      if (!("thunk" in routeKey)) return renderSection(router, depth, routeKey, parent);
      const boundary = routeKey;
      return computed(() => {
        trackLazySubtrees();
        if (boundary.error !== undefined) throw boundary.error;
        return undefined;
      });
    });
  });
}

function renderSection(
  router: RouterState,
  depth: number,
  key: RouteDefinition,
  parent: RouteContext,
): JSX.Element {
  const initial = untrack(router.matches);
  const matchesAtLevel = computed<RouteMatch[]>((previous) => {
    const current = router.matches();
    return current[depth]?.route.key === key ? current : (previous ?? initial);
  });
  const match = (): RouteMatch => matchesAtLevel()[depth]!;
  const { pattern, component, preload } = initial[depth]!.route;
  const path = computed(() => match().path);
  const params = router.wrapParams(() => mergeParams(matchesAtLevel()));
  const { location } = router;
  (component as { preload?: () => unknown } | undefined)?.preload?.();
  const preloaded = router.takePreloaded(key);
  const data = preloaded
    ? preloaded.data
    : preload && runPreload(preload, { params, location, intent: router.intent() ?? "initial" });
  const route: RouteContext = {
    parent,
    pattern,
    params,
    path,
    outlet: () => renderRoutes(router, depth + 1, route),
    resolvePath: (to) => resolvePath(router.base.path(), to, path()),
  };
  return RouteContextKey({
    value: route,
    get children() {
      if (component === undefined) return route.outlet();
      const props: RouteSectionProps = {
        params,
        location,
        data,
        get children() {
          return route.outlet();
        },
      };
      return createComponent(component as (props: RouteSectionProps) => JSX.Element, props);
    },
  });
}

export function useResolvedPath(path: () => string): () => string | undefined {
  const route = useRoute();
  return computed(() => route.resolvePath(path()));
}

export function useHref<T extends string | undefined>(to: () => T): () => string | T {
  const router = useRouter();
  return computed(() => {
    const target = to();
    return target !== undefined ? router.renderPath(target) : target;
  });
}

export function useNavigate(): Navigator {
  return useRouter().navigatorFactory();
}

export function useLocation<S = unknown>(): Location<S> {
  return useRouter().location as Location<S>;
}

export function useIsRouting(): () => boolean {
  return useRouter().isRouting;
}

/** Matches the pattern against the current pathname, without consulting the route tree. */
export function useMatch<S extends string | TypedPath>(
  path: () => S,
  matchFilters?: MatchFilters<S extends string ? S : string>,
): () => PathMatch<S extends string ? RouteParams<S> : Params> | undefined {
  const location = useLocation();
  const matchers = computed(() =>
    expandOptionals(String(path())).map((pattern) =>
      createMatcher(pattern, undefined, matchFilters as MatchFilters),
    ),
  );
  return computed(() => {
    for (const matcher of matchers()) {
      const match = matcher(location.pathname);
      if (match) return match as PathMatch<S extends string ? RouteParams<S> : Params>;
    }
    return undefined;
  });
}

export function useRouteMatches(): () => RouteMatch[] {
  const router = useRouter();
  return () => router.matches().slice();
}

export function usePreloadRoute(): (
  url: string | URL | TypedPath,
  options?: { preloadData?: boolean },
) => void {
  const router = useRouter();
  return (url, options = {}) =>
    router.preloadRoute(
      url instanceof URL ? url : new URL(String(url), mockBase),
      options.preloadData,
    );
}

export function useParams<T extends Params>(): T;
export function useParams<P extends Params>(path: TypedPath<P>): { [K in keyof P]: P[K] };
export function useParams(): Params {
  return useRoute().params;
}

type SetSearch<In> = (params: In, options?: Partial<NavigateOptions>) => void;

export function useSearchParams<In, Out>(
  path: TypedSearchPath<In, Out>,
): [Out, SetSearch<Partial<In>>];
export function useSearchParams<T extends SearchParams>(): [Partial<T>, SetSearch<SetSearchParams>];
export function useSearchParams(
  path?: TypedSearchPath,
): [SearchParams, SetSearch<SetSearchParams>] {
  const router = useRouter();
  const { location } = router;
  const navigate = router.navigatorFactory();
  const setSearchParams: SetSearch<SetSearchParams> = (params, options) => {
    navigate(
      ((headed: LocationChange) => {
        const url = toURL(headed.value);
        return url.pathname + mergeSearchString(url.search, params) + url.hash;
      }) as unknown as string,
      { scroll: false, resolve: false, ...options },
    );
  };
  if (!path) return [location.query, setSearchParams];
  const parsed = computed(() => {
    const raw = { ...location.query };
    let result: SearchParams | undefined;
    for (const match of router.matches()) {
      const schema = (match.route.key as RouteDefinition).search;
      if (!schema) continue;
      const outcome = validateSearch(schema, raw);
      if (!outcome.issues) result = Object.assign(result ?? { ...raw }, outcome.value);
    }
    return result ?? raw;
  });
  return [createMemoObject(parsed), setSearchParams];
}

export interface LinkState {
  active: () => boolean;
  current: () => boolean;
  pending: () => boolean;
}

export function useLinkState(
  href: () => string | TypedPath,
  options: { end?: boolean } = {},
): LinkState {
  const router = useRouter();
  const to = useResolvedPath(() => String(href()));
  const path = computed(() => {
    const resolved = to();
    return resolved === undefined ? undefined : comparablePath(resolved);
  });
  const matchesPath = (target: string): [isActive: boolean, isExact: boolean] => {
    const linkPath = path();
    if (linkPath === undefined) return [false, false];
    const isExact = target === linkPath;
    return [isExact || (!options.end && target.startsWith(linkPath + "/")), isExact];
  };
  const state = computed(() => matchesPath(decodeURI(comparablePath(router.location.pathname))));
  return {
    active: () => state()[0],
    current: () => state()[1],
    pending: computed(() => {
      const target = router.pendingTarget();
      return target !== undefined && matchesPath(decodeURI(comparablePath(target.value)))[0];
    }),
  };
}
