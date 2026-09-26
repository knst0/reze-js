// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { isServerRender } from "@rezejs/dom";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { computed, signal, untrack, useContext } from "@rezejs/signals";

import { setupLinkClaims, setupNativeEvents } from "./events";
import { browserHistory, type RouterHistory } from "./history";
import { createBranches, getRouteMatches, trackLazySubtrees } from "./matching";
import { createPathsProxy, HREF, type RoutePaths } from "./paths";
import {
  createRouterContext,
  renderRoutes,
  runPreload,
  RouterContext,
  type RouterIntegration,
  type RouterState,
} from "./routing";
import { createScrollRestoration, withScrollRestoration } from "./scrollRestoration";
import type {
  Branch,
  DefinedRouteFilters,
  LazyRouteChildren,
  Intent,
  Location,
  LocationChange,
  OutputMatch,
  Params,
  RouteDefinition,
  RouteInfo,
  RouteParams,
  RoutePreloadFunc,
  RoutePreloadFuncArgs,
  RouteSectionComponent,
  RouteSectionProps,
  StandardSchemaV1,
  TypedPath,
  ValidFilters,
} from "./types";
import { isSameLocationChange, mockBase } from "./utils";

export function defineRoutes<const R extends readonly RouteDefinition[]>(routes: R): R {
  return routes;
}

type RouteChildren = RouteDefinition | readonly RouteDefinition[] | LazyRouteChildren;

type DefinedRouteComponent<T, P extends Params> = (
  props: RouteSectionProps<T, P> & { children?: JSX.Element },
) => JSX.Element;

export type DefinedRoute<
  S = undefined,
  T = unknown,
  F = undefined,
  C = undefined,
  Sch = undefined,
> = ([S] extends [undefined] ? {} : { path: S }) &
  ([F] extends [undefined] ? {} : DefinedRouteFilters<S> extends F ? {} : { matchFilters: F }) &
  ([C] extends [undefined] ? {} : [RouteChildren | undefined] extends [C] ? {} : { children: C }) &
  ([Sch] extends [undefined] ? {} : { search: Sch }) & {
    component?: RouteSectionComponent<T> | undefined;
    preload?: RoutePreloadFunc<T> | undefined;
    info?: RouteInfo | undefined;
  };

export function defineRoute<
  const S extends string | readonly string[],
  T = unknown,
  const F = DefinedRouteFilters<S>,
  const C extends RouteChildren | undefined = RouteChildren | undefined,
  Sch extends StandardSchemaV1<unknown, unknown> | undefined = undefined,
>(route: {
  path: S;
  matchFilters?: (F & ValidFilters<F, S>) | undefined;
  preload?: ((args: RoutePreloadFuncArgs<RouteParams<S>>) => T) | undefined;
  component?: DefinedRouteComponent<T, RouteParams<S>> | undefined;
  children?: C;
  search?: Sch;
  info?: RouteInfo | undefined;
}): DefinedRoute<S, T, F, C, Sch>;
export function defineRoute<
  T = unknown,
  const C extends RouteChildren | undefined = RouteChildren | undefined,
  Sch extends StandardSchemaV1<unknown, unknown> | undefined = undefined,
>(route: {
  preload?: ((args: RoutePreloadFuncArgs) => T) | undefined;
  component?: DefinedRouteComponent<T, Params> | undefined;
  children?: C;
  search?: Sch;
  info?: RouteInfo | undefined;
}): DefinedRoute<undefined, T, undefined, C, Sch>;
export function defineRoute(route: RouteDefinition): RouteDefinition {
  return route;
}

export interface RouterConfig<R extends readonly RouteDefinition[] = RouteDefinition[]> {
  routes: R;
  base?: string;
  /** Runs once per mount to warm app-wide data; the result reaches the render-prop child as `props.data`. */
  preload?: RoutePreloadFunc;
  /** Client history adapter, defaulting to browser history. On the server only its `utils` apply. */
  history?: RouterHistory;
  singleFlight?: boolean;
  actionBase?: string;
  explicitLinks?: boolean;
  /** Preload route code/data on link hover and focus. Defaults to `true`. */
  preloadLinks?: boolean;
  /**
   * Back/forward scroll restoration, replacing the browser heuristic that loses offsets while
   * the next page renders. Defaults to `true` with the default browser history; a custom
   * history adapter must opt in explicitly.
   */
  scrollRestoration?: boolean;
  transformUrl?: (url: string) => string;
}

export interface RouterProps {
  /**
   * Server-only: the location for this render (SSG scripts, tests). Ignored on the client,
   * where the history adapter owns the location.
   */
  url?: string;
  children?: (props: RouteSectionProps) => JSX.Element;
}

export interface RouterInstance<R extends readonly RouteDefinition[] = RouteDefinition[]> {
  (props: RouterProps): JSX.Element;
  /** Typed path proxy — builds URLs through property access and calls. */
  readonly paths: RoutePaths<R>;
  readonly routes: R;
  /** The config the instance was created with. */
  readonly config: RouterConfig<R>;
  /** Pure matching against an arbitrary URL — root to leaf, `[]` when nothing matches. */
  match(url: string): OutputMatch[];
}

function normalizeChange(value: string | LocationChange): LocationChange {
  return typeof value === "string" ? { value } : value;
}

function staticIntegration(url: string | undefined): RouterIntegration {
  const source: LocationChange = { value: "" };
  if (url) {
    try {
      const parsed = new URL(url, mockBase);
      source.value = parsed.pathname + parsed.search;
    } catch {
      source.value = url;
    }
  }
  return {
    signal: [
      () => source,
      (next) => {
        if (!isSameLocationChange(next, source)) Object.assign(source, next);
      },
    ],
  };
}

function clientIntegration(history: RouterHistory): RouterIntegration {
  const [source, setSource] = signal<LocationChange>(normalizeChange(history.get()), {
    equals: (a, b) => isSameLocationChange(a, b),
  });
  let isCommitting = false;
  return {
    signal: [
      source,
      (next) => setSource((current) => (isSameLocationChange(next, current) ? current : next)),
    ],
    commit: (next) => {
      isCommitting = true;
      try {
        history.set(next);
      } finally {
        isCommitting = false;
      }
    },
    listen: history.init
      ? (notify) =>
          history.init!((value) => {
            if (isCommitting) return;
            notify(normalizeChange(value ?? history.get()));
          })
      : undefined,
    utils: history.utils,
  };
}

function Root(props: {
  router: RouterState;
  root: ((props: RouteSectionProps) => JSX.Element) | undefined;
  preload: RoutePreloadFunc | undefined;
}): JSX.Element {
  const { router } = props;
  const data = computed(() =>
    props.preload
      ? untrack(() =>
          runPreload(props.preload!, {
            params: router.params,
            location: router.location,
            intent: router.intent() ?? ("initial" as Intent),
          }),
        )
      : undefined,
  );
  const location: Location = router.location;
  if (!props.root) return renderRoutes(router, 0, router.base);
  const section: RouteSectionProps = {
    params: router.params,
    location,
    data: data(),
    get children() {
      return renderRoutes(router, 0, router.base);
    },
  };
  return props.root(section);
}

export function createRouter<const R extends readonly RouteDefinition[]>(
  config: RouterConfig<R>,
): RouterInstance<R> {
  const basePath = config.base ?? "";
  let compiled: Branch[] | undefined;
  let compiledVersion = -1;
  const branches = (): Branch[] => {
    const version = trackLazySubtrees();
    if (!compiled || compiledVersion !== version) {
      compiled = createBranches(config.routes, basePath);
      compiledVersion = version;
    }
    return compiled;
  };
  const renderPath = config.history?.utils?.renderPath;
  const matchPath = (pathname: string): OutputMatch[] =>
    getRouteMatches(branches(), config.transformUrl ? config.transformUrl(pathname) : pathname).map(
      ({ route, path, params }) => ({
        path: route.originalPath,
        pattern: route.pattern,
        match: path,
        params,
        info: route.info,
      }),
    );
  const pathnameOf = (url: string | TypedPath): string =>
    typeof url === "string"
      ? new URL(url, mockBase).pathname
      : ((url as unknown as Record<typeof HREF, string>)[HREF] as string);

  function RouterComponent(props: RouterProps): JSX.Element {
    if (process.env.NODE_ENV !== "production" && useContext(RouterContext) !== undefined) {
      console.warn(
        "Mounting a router inside another router is not supported. " +
          "Compose route trees in one createRouter config instead.",
      );
    }
    const server = isServerRender();
    const root = untrack(() => props.children);
    let history = config.history;
    const restoration =
      !server && (config.scrollRestoration ?? !history) ? createScrollRestoration() : undefined;
    if (restoration) history = withScrollRestoration(history ?? browserHistory(), restoration);
    const integration = server
      ? staticIntegration(props.url)
      : clientIntegration(history ?? browserHistory());
    const router = createRouterContext(integration, branches, {
      base: basePath,
      singleFlight: config.singleFlight,
      transformUrl: config.transformUrl,
      isServer: server,
    });
    if (!server) {
      setupNativeEvents({
        preload: config.preloadLinks,
        explicitLinks: config.explicitLinks,
        actionBase: config.actionBase,
        transformUrl: config.transformUrl,
      })(router);
      setupLinkClaims(router, config.explicitLinks);
      restoration?.create(router);
    }
    return RouterContext({
      value: router,
      get children() {
        return Root({ router, root, preload: config.preload });
      },
    });
  }

  const instance = RouterComponent as RouterInstance<R>;
  (instance as { routes: R }).routes = config.routes;
  (instance as { config: RouterConfig<R> }).config = config;
  instance.match = (url: string): OutputMatch[] => matchPath(pathnameOf(url));
  let paths: RouterInstance<R>["paths"] | undefined;
  Object.defineProperty(instance, "paths", {
    get: () => (paths ??= createPathsProxy(renderPath, basePath)),
  });
  return instance;
}
