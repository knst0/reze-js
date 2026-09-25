import { createComponent, createContext, startTransition, useContext } from "@rezejs/dom";
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

const RouteMark = Symbol("route");

interface RouteDefinition {
  [RouteMark]: true;
  props: RouteProps;
}

/**
 * A data-driven route for `Router(routes)`: same `path` language as `<Route>`, `(group)`
 * segments stripped, `""` nesting without adding a segment. File-system manifests
 * (`filesystem-routing` neutral paths, nested via its `buildRouteTree`) map here directly.
 */
export interface RouteConfig {
  path: string;
  component?: Component;
  children?: RouteConfig[];
}

interface RouteNode {
  path: string;
  component?: Component;
  children: RouteNode[];
}

/** Declares a route inside `<Router>` or another `<Route>`; renders nothing by itself. */
export function Route(props: RouteProps): JSX.Element {
  return { [RouteMark]: true, props } as unknown as JSX.Element;
}

interface Branch {
  pattern: Pattern;
  routes: RouteNode[];
  order: number;
}

function definitions(children: unknown): RouteDefinition[] {
  const list = Array.isArray(children) ? children.flat(Infinity) : [children];
  return list.filter(
    (item): item is RouteDefinition =>
      typeof item === "object" && item !== null && RouteMark in item,
  );
}

function jsxNodes(defs: RouteDefinition[]): RouteNode[] {
  return defs.map((def) => ({
    path: def.props.path,
    component: def.props.component,
    children: jsxNodes(definitions(untrack(() => def.props.children))),
  }));
}

function configNodes(configs: RouteConfig[]): RouteNode[] {
  return configs.map((config) => ({
    path: config.path,
    component: config.component,
    children: config.children === undefined ? [] : configNodes(config.children),
  }));
}

function joinPaths(parent: string, child: string): string {
  return `${parent.replace(/\/+$/, "")}/${child.replace(/^\/+/, "")}`;
}

function stripGroups(path: string): string {
  const stripped = path.replace(/\/\([^/()]+\)/g, "");
  return stripped === "" ? "/" : stripped;
}

function branchesOf(
  routes: RouteNode[],
  parentPath: string,
  parents: RouteNode[],
  out: Branch[],
): Branch[] {
  for (const route of routes) {
    const path = stripGroups(joinPaths(parentPath, route.path));
    const chain = [...parents, route];
    out.push({ pattern: compilePattern(path), routes: chain, order: out.length });
    branchesOf(route.children, path, chain, out);
  }
  return out;
}

interface Match {
  routes: RouteNode[];
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
  if (base === "" || !pathname.startsWith(base)) return pathname;
  return pathname.slice(base.length) || "/";
}

export interface RouterProps {
  /** The URL to render on the server; in the browser the current location is used. */
  url?: string;
  /** A path prefix every route lives under, e.g. `/docs`; write it into link hrefs. */
  base?: string;
  /** Data-driven routes, combined with `<Route>` children; `fileRoutes` builds them. */
  routes?: RouteConfig[];
  children?: JSX.Element;
}

function isBrowser(url: string | undefined): boolean {
  return url === undefined && typeof window !== "undefined";
}

/**
 * Renders the route matching the current location. In the browser it follows the History API:
 * `navigate` and plain left clicks on native `<a href>` (same origin, no modifier keys, no
 * `target`/`download`) first load the code of lazy route components (the current page stays
 * meanwhile, `useIsRouting()` is true), then push an entry and change the location inside a
 * transition. A push scrolls to the top; back/forward restores the scroll position. On the
 * server, `url` is the location.
 */
export function Router(props: RouterProps): JSX.Element {
  const base = (props.base ?? "").replace(/\/+$/, "");
  const inBrowser = isBrowser(props.url);
  const current = (): Location =>
    inBrowser ? parseLocation(window.location.href) : parseLocation(props.url ?? "/");
  const [location, setLocation] = signal(current(), {
    equals: (a, b) => a.pathname === b.pathname && a.search === b.search && a.hash === b.hash,
  });
  const branches = computed(() => {
    const roots = [
      ...jsxNodes(definitions(props.children)),
      ...configNodes(props.routes ?? []),
    ];
    return branchesOf(roots, "/", [], []);
  });
  const match = computed(() => matchBranches(branches(), withoutBase(location().pathname, base)));
  const [isRouting, setRouting] = signal(false);
  let latest = 0;
  const commit = async (target: Location, write: () => void): Promise<boolean> => {
    const navigation = ++latest;
    setRouting(true);
    const routes =
      matchBranches(untrack(branches), withoutBase(target.pathname, base))?.routes ?? [];
    const preloads = routes.flatMap((route) => {
      const preload = (route.component as { preload?: () => Promise<unknown> } | undefined)
        ?.preload;
      return preload === undefined ? [] : [preload()];
    });
    await Promise.all(preloads).catch(() => {});
    if (navigation !== latest) return false;
    write();
    await startTransition(() => setLocation(target));
    if (navigation === latest) setRouting(false);
    return true;
  };
  const resolveTarget = (to: string, here: Location): Location => {
    if (!to.startsWith("/")) {
      return parseLocation(new URL(to, `http://localhost${here.pathname}${here.search}`).href);
    }
    if (base !== "" && to !== base && !to.startsWith(`${base}/`)) {
      return parseLocation(joinPaths(base || "/", to));
    }
    return parseLocation(to);
  };
  const navigate = async (to: string, options?: NavigateOptions): Promise<void> => {
    const here = untrack(location);
    const target = resolveTarget(to, here);
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
    if (isCommitted && !options?.replace) window.scrollTo(0, 0);
  };
  if (inBrowser) {
    const onClick = (event: MouseEvent): void => {
      if (event.button !== 0 || event.defaultPrevented) return;
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
      const anchor = (event.target as Element | null)?.closest?.("a[href]") as
        | HTMLAnchorElement
        | null
        | undefined;
      if (anchor === null || anchor === undefined) return;
      if (anchor.target !== "" && anchor.target !== "_self") return;
      if (anchor.hasAttribute("download")) return;
      if (anchor.origin !== window.location.origin) return;
      const href = anchor.getAttribute("href");
      if (href === null || href === "" || href.startsWith("#")) return;
      if (/^(mailto|tel|javascript|data|blob):/i.test(href)) return;
      event.preventDefault();
      void navigate(href);
    };
    const onPopState = (event: PopStateEvent): void => {
      void commit(current(), () => {}).then((isCommitted) => {
        const scroll = (event.state as { scroll?: number } | null)?.scroll;
        if (isCommitted && typeof scroll === "number") window.scrollTo(0, scroll);
      });
    };
    document.addEventListener("click", onClick);
    window.addEventListener("popstate", onPopState);
    onCleanup(() => {
      document.removeEventListener("click", onClick);
      window.removeEventListener("popstate", onPopState);
    });
  }
  const state: RouterState = { location, match, isRouting, base, navigate };
  return createComponent(RouterContext, {
    value: state,
    get children() {
      return createComponent(DepthContext, {
        value: 0,
        get children() {
          return createComponent(Outlet, {});
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
          const component = current.component;
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

/** Whether the current pathname equals `path`, for marking native links. */
export function useMatch(path: string): Getter<boolean> {
  const router = useRouter();
  const pathname = parseLocation(joinPaths(router.base || "/", path)).pathname;
  return () => router.location().pathname === pathname;
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
