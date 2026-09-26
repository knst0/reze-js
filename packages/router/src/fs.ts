// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { lazy } from "@rezejs/dom";

import type {
  DefinedRouteFilters,
  RouteInfo,
  RouteParams,
  RoutePreloadFunc,
  RoutePreloadFuncArgs,
  RouteSectionComponent,
  StandardSchemaV1,
  TypedRouteConfig,
  ValidFilters,
} from "./types";

/** The type `defineFileRoute` hands back: filters and search stay literal for `paths`. */
export type FileRouteConfig<
  S extends string = string,
  T = unknown,
  F = undefined,
  Sch = undefined,
> = TypedRouteConfig<S> &
  ([F] extends [undefined] ? {} : DefinedRouteFilters<S> extends F ? {} : { matchFilters: F }) &
  ([Sch] extends [undefined] ? {} : { search: Sch }) & {
    preload?: RoutePreloadFunc<T> | undefined;
    info?: RouteInfo | undefined;
  };

/**
 * Types a route file's `route` export from a path-pattern witness: `preload` params come from
 * the pattern, while the manifest still supplies the runtime path.
 */
export function defineFileRoute<
  S extends string,
  T = unknown,
  const F = DefinedRouteFilters<S>,
  Sch extends StandardSchemaV1<unknown, unknown> | undefined = undefined,
>(
  path: S,
  config: {
    matchFilters?: (F & ValidFilters<F, S>) | undefined;
    preload?: ((args: RoutePreloadFuncArgs<RouteParams<S>>) => T) | undefined;
    search?: Sch;
    info?: RouteInfo | undefined;
  },
): FileRouteConfig<S, T, F, Sch> {
  return config as unknown as FileRouteConfig<S, T, F, Sch>;
}

/** A code-split module ref: delivered as a dynamic import. */
export interface FileRouteLazyRef<M = Record<string, unknown>> {
  src: string;
  import(): Promise<M>;
}

/** An eager module ref: its picked exports are imported statically. */
export interface FileRouteEagerRef<M = Record<string, unknown>> {
  src?: string | undefined;
  require(): M;
}

/** One nested route-manifest entry the adapter consumes. */
export interface FileRouteEntry {
  path: string;
  page?: boolean;
  $component?: FileRouteLazyRef<unknown> | FileRouteEagerRef<unknown> | undefined;
  $$route?: FileRouteEagerRef<unknown> | undefined;
  children?: readonly FileRouteEntry[] | undefined;
}

type RouteConfigOf<E> = E extends { $$route: { require(): { route: infer R } } } ? R : {};

/**
 * One manifest entry as a route definition. `children` is a required key on purpose: an
 * optional key infers as `C | undefined`, distributing `RoutePaths` into an uncallable shape.
 */
export type FileRouteFrom<E> = RouteConfigOf<E> & {
  path: E extends { path: infer P extends string } ? P : never;
  component: E extends { $component: object } ? RouteSectionComponent : undefined;
  children: E extends { children: infer C extends readonly FileRouteEntry[] }
    ? FileRoutesFrom<C>
    : undefined;
};

export type FileRoutesFrom<T extends readonly FileRouteEntry[]> = {
  [K in keyof T]: FileRouteFrom<T[K]>;
};

function componentOf(
  entry: FileRouteEntry,
  components: Map<string, RouteSectionComponent>,
): RouteSectionComponent | undefined {
  const ref = entry.$component;
  if (!ref) return undefined;
  if ("require" in ref && typeof ref.require === "function") {
    return (ref.require() as { default?: RouteSectionComponent }).default;
  }
  if (!("import" in ref) || typeof ref.import !== "function") return undefined;
  const importRef = ref as FileRouteLazyRef;
  const cached = components.get(importRef.src);
  if (cached) return cached;
  const component = lazy(() => importRef.import() as Promise<{ default: RouteSectionComponent }>);
  components.set(importRef.src, component as unknown as RouteSectionComponent);
  return component as unknown as RouteSectionComponent;
}
interface FileRouteNode {
  path: string;
  component: RouteSectionComponent | undefined;
  info: RouteInfo;
  children: FileRouteNode[] | undefined;
  [key: string]: unknown;
}

function toRoute(
  entry: FileRouteEntry,
  components: Map<string, RouteSectionComponent>,
): FileRouteNode {
  const required = entry.$$route?.require() as { route?: Record<string, unknown> } | undefined;
  const config = required?.route ?? {};
  return {
    ...config,
    path: entry.path,
    component: componentOf(entry, components),
    info: {
      ...(config as { info?: RouteInfo }).info,
      filesystem: true,
    },
    children: entry.children?.map((child) => toRoute(child, components)),
  };
}

export function fileRoutes<const T extends readonly FileRouteEntry[]>(
  entries: T,
): FileRoutesFrom<T> {
  const components = new Map<string, RouteSectionComponent>();
  return entries.map((entry) => toRoute(entry, components)) as FileRoutesFrom<T>;
}
