import { createComponent, createContext, lazy, startTransition, useContext } from "@rezejs/dom";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { computed, onCleanup, signal, untrack, type Getter } from "@rezejs/signals";

import { compilePattern, matchPattern, type Pattern } from "./match";

// oxlint-disable-next-line typescript/no-explicit-any
type Component = (props: any) => JSX.Element;

export interface RouteProps {
  /** `/users/:id`, `:id?` for an optional segment, `*rest` for the remaining ones. */
  path: string;
  /** Rendered for this route; nested routes render in its `<Outlet />`. Without one, the outlet alone. */
  component?: Component;
  children?: JSX.Element;
}

/** A route as data, e.g. from `virtual:reze-routes`: `load` imports a module whose default export is the component. */
export interface RouteConfig {
  path: string;
  component?: Component;
  load?: () => Promise<{ default: Component }>;
  children?: RouteConfig[];
}

const RouteMark = Symbol("route");

interface RouteDefinition {
  [RouteMark]: true;
  props: RouteProps;
}

/** Declares a route inside `<Router>` or another `<Route>`; renders nothing by itself. */
export function Route(props: RouteProps): JSX.Element {
  return { [RouteMark]: true, props } as unknown as JSX.Element;
}

const configDefinitions = new WeakMap<RouteConfig, RouteDefinition>();

function configDefinition(config: RouteConfig): RouteDefinition {
  let definition = configDefinitions.get(config);
  if (definition === undefined) {
    const component = config.component ?? (config.load && lazy(config.load));
    definition = {
      [RouteMark]: true,
      props: { path: config.path, component, children: config.children as JSX.Element },
    };
    configDefinitions.set(config, definition);
  }
  return definition;
}

function isRouteConfig(item: object): item is RouteConfig {
  return typeof (item as RouteConfig).path === "string" && !(RouteMark in item);
}

interface Branch {
  pattern: Pattern;
  routes: RouteDefinition[];
  order: number;
}

function definitions(children: unknown): RouteDefinition[] {
  const list = Array.isArray(children) ? children.flat(Infinity) : [children];
  return list.flatMap((item): RouteDefinition[] => {
    if (typeof item !== "object" || item === null) return [];
    if (RouteMark in item) return [item as RouteDefinition];
    return isRouteConfig(item) ? [configDefinition(item)] : [];
  });
}

function joinPaths(parent: string, child: string): string {
  return `${parent.replace(/\/+$/, "")}/${child.replace(/^\/+/, "")}`;
}

function branchesOf(
  routes: RouteDefinition[],
  parentPath: string,
  parents: RouteDefinition[],
  out: Branch[],
): Branch[] {
  for (const route of routes) {
    const path = joinPaths(parentPath, route.props.path);
    const chain = [...parents, route];
    out.push({ pattern: compilePattern(path), routes: chain, order: out.length });
    branchesOf(definitions(untrack(() => route.props.children)), path, chain, out);
  }
  return out;
}

interface Match {
  routes: RouteDefinition[];
  params: Record<string, string>;
}

function matchBranches(branches: Branch[], path: string): Match | undefined {
  let best: { branch: Branch; params: Record<string, string> } | undefined;
  for (const branch of branches) {
    const params = matchPattern(branch.pattern, path);
    if (params === undefined) continue;
    const score = branch.pattern.score;
    const bestScore = best?.branch.pattern.score ?? -1;
    const isDeeperTie =
      score === bestScore && branch.routes.length > (best?.branch.routes.length ?? 0);
    if (score > bestScore || isDeeperTie) {
      best = { branch, params };
    }
  }
  return best && { routes: best.branch.routes, params: best.params };
}

export interface Location {
  readonly pathname: string;
  readonly search: string;
  readonly hash: string;
}

export interface NavigateOptions {
  /** Replace the current history entry instead of pushing one. */
  replace?: boolean;
  /** Scroll to the top after a push; defaults to `true`. */
  scroll?: boolean;
}

interface RouterState {
  location: Getter<Location>;
  match: Getter<Match | undefined>;
  isRouting: Getter<boolean>;
  base: string;
  navigate: (to: string, options?: NavigateOptions) => Promise<void>;
}

const RouterContext = createContext<RouterState | undefined>(undefined);
const DepthContext = createContext(0);

function parseLocation(url: string): Location {
  const parsed = new URL(url, "http://localhost");
  return { pathname: parsed.pathname, search: parsed.search, hash: parsed.hash };
}

function withoutBase(pathname: string, base: string): string {
  if (base === "" || !isUnderBase(pathname, base)) return pathname;
  return pathname.slice(base.length) || "/";
}

export interface RouterProps {
  /** The URL to render on the server; in the browser the current location is used. */
  url?: string;
  /** A path prefix every route and link lives under, e.g. `/docs`. */
  base?: string;
  /** Routes as data, e.g. `virtual:reze-routes`; used instead of the `<Route>` children. */
  routes?: RouteConfig[];
  /** A layout around every route: it gets the matched route as `children`. */
  root?: (props: { children: JSX.Element }) => JSX.Element;
  children?: JSX.Element;
}

function normalizeBase(base: string | undefined): string {
  return (base ?? "").replace(/\/+$/, "");
}

function isUnderBase(pathname: string, base: string): boolean {
  return base === "" || pathname === base || pathname.startsWith(`${base}/`);
}

function isBrowser(url: string | undefined): boolean {
  return url === undefined && typeof window !== "undefined";
}

/**
 * Renders the route matching the current location. In the browser it follows the History API:
 * `navigate` and clicks on native `<a>` elements first load the code of lazy route components
 * (the current page stays meanwhile, `useIsRouting()` is true), then push an entry and change
 * the location inside a transition. A push scrolls to the top; back/forward restores the scroll
 * position. On the server, `url` is the location.
 *
 * A plain left click on a same-origin `<a href>` under `base` navigates without reloading,
 * unless the link has `target` (other than `_self`), `download`, `rel="external"`, or only
 * changes the hash of the current page. `<a replace>` replaces the history entry,
 * `<a noscroll>` keeps the scroll position.
 */
export function Router(props: RouterProps): JSX.Element {
  const base = normalizeBase(props.base);
  const inBrowser = isBrowser(props.url);
  const current = (): Location =>
    inBrowser ? parseLocation(window.location.href) : parseLocation(props.url ?? "/");
  const [location, setLocation] = signal(current(), {
    equals: (a, b) => a.pathname === b.pathname && a.search === b.search && a.hash === b.hash,
  });
  const branches = computed(() =>
    branchesOf(definitions(props.routes ?? props.children), "/", [], []),
  );
  const match = computed(() => matchBranches(branches(), withoutBase(location().pathname, base)));
  const [isRouting, setRouting] = signal(false);
  let latest = 0;
  const commit = async (target: Location, write: () => void): Promise<boolean> => {
    const navigation = ++latest;
    setRouting(true);
    await preloadMatched(untrack(branches), withoutBase(target.pathname, base)).catch(() => {});
    if (navigation !== latest) return false;
    write();
    await startTransition(() => setLocation(target));
    if (navigation === latest) setRouting(false);
    return true;
  };
  const navigate = async (to: string, options?: NavigateOptions): Promise<void> => {
    const here = untrack(location);
    const target = to.startsWith("/")
      ? parseLocation(joinPaths(base || "/", to))
      : parseLocation(new URL(to, `http://localhost${here.pathname}${here.search}`).href);
    if (!inBrowser) {
      await commit(target, () => {});
      return;
    }
    const url = target.pathname + target.search + target.hash;
    const isCommitted = await commit(target, () => {
      if (options?.replace) {
        window.history.replaceState({ scroll: 0 }, "", url);
      } else {
        window.history.replaceState({ ...window.history.state, scroll: window.scrollY }, "");
        window.history.pushState({ scroll: 0 }, "", url);
      }
    });
    if (isCommitted && !options?.replace && options?.scroll !== false) window.scrollTo(0, 0);
  };
  if (inBrowser) {
    const onPopState = (event: PopStateEvent): void => {
      void commit(current(), () => {}).then((isCommitted) => {
        const scroll = (event.state as { scroll?: number } | null)?.scroll;
        if (isCommitted && typeof scroll === "number") window.scrollTo(0, scroll);
      });
    };
    const onClick = (event: MouseEvent): void => {
      const anchor = linkOf(event);
      if (anchor === undefined || !isPlainLeftClick(event, anchor)) return;
      const url = new URL(anchor.href);
      const here = untrack(location);
      const isHashJump =
        url.hash !== "" && url.pathname === here.pathname && url.search === here.search;
      if (!isUnderBase(url.pathname, base) || isHashJump) return;
      event.preventDefault();
      void navigate(withoutBase(url.pathname, base) + url.search + url.hash, {
        replace: anchor.hasAttribute("replace"),
        scroll: !anchor.hasAttribute("noscroll"),
      });
    };
    window.addEventListener("popstate", onPopState);
    window.addEventListener("click", onClick);
    onCleanup(() => {
      window.removeEventListener("popstate", onPopState);
      window.removeEventListener("click", onClick);
    });
  }
  const state: RouterState = { location, match, isRouting, base, navigate };
  return createComponent(RouterContext, {
    value: state,
    get children() {
      return createComponent(DepthContext, {
        value: 0,
        get children() {
          const root = props.root;
          if (root === undefined) return createComponent(Outlet, {});
          return createComponent(root, {
            get children() {
              return createComponent(Outlet, {});
            },
          });
        },
      });
    },
  });
}

function useRouter(): RouterState {
  const router = useContext(RouterContext);
  if (router === undefined) throw new Error("router hooks must be used inside <Router>");
  return router;
}

/** Renders the component of the matched route one level below the enclosing one. */
export function Outlet(): JSX.Element {
  const router = useRouter();
  const depth = useContext(DepthContext);
  const route = computed(() => router.match()?.routes[depth]);
  return computed(() => {
    const current = route();
    if (current === undefined) return undefined;
    return untrack(() =>
      createComponent(DepthContext, {
        value: depth + 1,
        get children() {
          const component = current.props.component;
          return component === undefined
            ? createComponent(Outlet, {})
            : createComponent(component, {});
        },
      }),
    );
  });
}

/** The params of the matched route, read reactively: `useParams().id`. */
export function useParams<T extends Record<string, string> = Record<string, string>>(): T {
  const router = useRouter();
  const params = (): Record<string, string> => router.match()?.params ?? {};
  return new Proxy({} as T, {
    get: (_, key) => (typeof key === "string" ? params()[key] : undefined),
    has: (_, key) => typeof key === "string" && key in params(),
    ownKeys: () => Object.keys(params()),
    getOwnPropertyDescriptor: (_, key) =>
      typeof key === "string" && key in params()
        ? { configurable: true, enumerable: true, value: params()[key] }
        : undefined,
  });
}

/** The current location, read reactively: `useLocation().pathname`. */
export function useLocation(): Location {
  const router = useRouter();
  return {
    get pathname() {
      return withoutBase(router.location().pathname, router.base);
    },
    get search() {
      return router.location().search;
    },
    get hash() {
      return router.location().hash;
    },
  };
}

/** Whether a navigation is loading the code of the routes it goes to. */
export function useIsRouting(): Getter<boolean> {
  return useRouter().isRouting;
}

/**
 * `navigate(to, { replace })`: `to` is absolute, or relative to the current path. The code of
 * lazy route components is loaded before the location changes, so the current page stays until
 * then; the returned promise settles once the new page's pending work did.
 */
export function useNavigate(): (to: string, options?: NavigateOptions) => Promise<void> {
  return useRouter().navigate;
}

function linkOf(event: MouseEvent): HTMLAnchorElement | undefined {
  for (const target of event.composedPath()) {
    if (target instanceof HTMLAnchorElement && target.hasAttribute("href")) return target;
  }
  return undefined;
}

function isPlainLeftClick(event: MouseEvent, anchor: HTMLAnchorElement): boolean {
  return (
    event.button === 0 &&
    !event.defaultPrevented &&
    !(event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) &&
    (!anchor.target || anchor.target === "_self") &&
    !anchor.hasAttribute("download") &&
    !anchor.relList.contains("external") &&
    anchor.origin === window.location.origin
  );
}

/**
 * The params `path()` binds for the current location, or `undefined` while it does not match:
 * `aria-current={useMatch(() => "/users/*")() ? "page" : undefined}`.
 */
export function useMatch(path: () => string): Getter<Record<string, string> | undefined> {
  const router = useRouter();
  const pattern = computed(() => compilePattern(path()));
  return computed(() =>
    matchPattern(pattern(), withoutBase(router.location().pathname, router.base)),
  );
}

/**
 * Loads the code of the lazy components `url` renders, so that server rendering and hydration
 * show them at once. `routes` is what `<Router routes>` gets.
 */
export function preloadRoutes(routes: RouteConfig[], url: string, base?: string): Promise<void> {
  const branches = branchesOf(definitions(routes), "/", [], []);
  return preloadMatched(branches, withoutBase(parseLocation(url).pathname, normalizeBase(base)));
}

async function preloadMatched(branches: Branch[], path: string): Promise<void> {
  const routes = matchBranches(branches, path)?.routes ?? [];
  await Promise.all(
    routes.flatMap((route) => {
      const preload = (route.props.component as { preload?: () => Promise<unknown> } | undefined)
        ?.preload;
      return preload === undefined ? [] : [preload()];
    }),
  );
}
