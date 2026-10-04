import type { Awaitable, PageMetadata, PreloadArgs, RouteComponent, RouteDefinition, RouteParams, RouteRedirect, RouteResolvedArgs } from "./types";

/** Preserves the literal route tuple for the router factory; returns the same value. */
export function defineRoutes<const R extends readonly RouteDefinition[]>(routes: R): R {
  return routes;
}

/** Returns the supplied route while typing its preload and component params from its path. */
export function defineRoute<const S extends string, T>(route: {
  path: S;
  preload?: (args: PreloadArgs<RouteParams<S>>) => T;
  meta?: PageMetadata | ((args: RouteResolvedArgs<RouteParams<S>, T>) => Awaitable<PageMetadata>);
  redirect?: RouteRedirect | ((args: RouteResolvedArgs<RouteParams<S>, T>) => Awaitable<RouteRedirect | undefined>);
  component?: RouteComponent<RouteParams<S>, Awaited<T>>;
  info?: Readonly<Record<string, unknown>>;
  children?: readonly RouteDefinition[];
}): RouteDefinition & { path: S } {
  return route;
}
