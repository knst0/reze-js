import type { PreloadArgs, RouteComponent, RouteDefinition, RouteParams } from "./types";

/** Preserves the literal route tuple for the router factory; returns the same value. */
export function defineRoutes<const R extends readonly RouteDefinition[]>(routes: R): R {
  return routes;
}

/** Returns the supplied route while typing its preload and component params from its path. */
export function defineRoute<const S extends string, T>(route: {
  path: S;
  preload?: (args: PreloadArgs<RouteParams<S>>) => T;
  component?: RouteComponent<RouteParams<S>, T>;
  info?: Readonly<Record<string, unknown>>;
  children?: readonly RouteDefinition[];
}): RouteDefinition & { path: S } {
  return route;
}
