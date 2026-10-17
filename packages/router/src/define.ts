import type {
  Awaitable,
  PreloadArgs,
  RouteComponent,
  RouteDefinition,
  RouteParams,
  RouteRedirect,
  RouteResolvedArgs,
  RouterContext,
} from "./types";

/** Preserves the literal route tuple for the router factory; returns the same value. */
export function defineRoutes<const R extends readonly RouteDefinition<any>[]>(routes: R): R {
  return routes;
}

type RouteInput<S extends string, T, C> = Omit<RouteDefinition<C>, "path" | "beforeLoad" | "preload" | "redirect" | "component"> & {
  path: S;
  beforeLoad?: (args: PreloadArgs<RouteParams<S>, C>) => Awaitable<Partial<C> | undefined | void>;
  preload?: (args: PreloadArgs<RouteParams<S>, C>) => T;
  redirect?: RouteRedirect | ((args: RouteResolvedArgs<RouteParams<S>, T, C>) => Awaitable<RouteRedirect | undefined>);
  component?: RouteComponent<RouteParams<S>, Awaited<T>>;
};

type PreloadedRoute<S extends string, T, C> = RouteInput<S, T, C> & {
  preload: (args: PreloadArgs<RouteParams<S>, C>) => T;
};

export function defineRoute<const S extends string, T, C = RouterContext>(
  route: PreloadedRoute<S, T, C>,
): Omit<RouteDefinition<C>, "path" | "preload"> & Pick<PreloadedRoute<S, T, C>, "path" | "preload">;
export function defineRoute<const S extends string, T = unknown, C = RouterContext>(
  route: RouteInput<S, T, C>,
): RouteDefinition<C> & { path: S };
export function defineRoute(route: RouteDefinition<any>): RouteDefinition<any> {
  return route;
}
