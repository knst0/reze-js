import type {
  Awaitable,
  PageMetadata,
  PreloadArgs,
  RouteComponent,
  RouteDefinition,
  RouteParams,
  RouteRedirect,
  RouteResolvedArgs,
} from "./types";

/** Preserves the literal route tuple for the router factory; returns the same value. */
export function defineRoutes<const R extends readonly RouteDefinition[]>(routes: R): R {
  return routes;
}

type RouteInput<S extends string, T> = Omit<RouteDefinition, "path" | "preload" | "meta" | "redirect" | "component"> & {
  path: S;
  preload?: (args: PreloadArgs<RouteParams<S>>) => T;
  meta?: PageMetadata | ((args: RouteResolvedArgs<RouteParams<S>, T>) => Awaitable<PageMetadata>);
  redirect?: RouteRedirect | ((args: RouteResolvedArgs<RouteParams<S>, T>) => Awaitable<RouteRedirect | undefined>);
  component?: RouteComponent<RouteParams<S>, Awaited<T>>;
};

type PreloadedRoute<S extends string, T> = RouteInput<S, T> & {
  preload: (args: PreloadArgs<RouteParams<S>>) => T;
};

export function defineRoute<const S extends string, T>(
  route: PreloadedRoute<S, T>,
): Omit<RouteDefinition, "path" | "preload"> & Pick<PreloadedRoute<S, T>, "path" | "preload">;
export function defineRoute<const S extends string, T = unknown>(route: RouteInput<S, T>): RouteDefinition & { path: S };
export function defineRoute(route: RouteDefinition): RouteDefinition {
  return route;
}
