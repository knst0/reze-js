// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import type { JSX } from "@rezejs/dom/jsx-runtime";

/** Augmentable route metadata, read via `info`, `Router.match` and `useRouteMatches`. */
export interface RouteInfo {
  [key: string]: unknown;
}

// oxlint-disable-next-line typescript/no-explicit-any -- component props are contravariant; `any` accepts every route component
export type Component<P = any> = (props: P) => JSX.Element;

export type Params = Record<string, string | undefined>;
export type SearchParams = Record<string, string | string[] | undefined>;
export type SetParams = Record<string, string | number | boolean | null | undefined>;
export type SetSearchParams = Record<
  string,
  string | string[] | number | number[] | boolean | boolean[] | null | undefined
>;

declare const PathParamsBrand: unique symbol;
declare const PathSearchBrand: unique symbol;
declare const RoutePatternBrand: unique symbol;

export interface TypedPath<P extends Params = Params> {
  readonly [PathParamsBrand]: P;
  toString(): string;
}

export interface TypedSearchPath<In = SetSearchParams, Out = SearchParams> {
  readonly [PathSearchBrand]: { input: In; output: Out };
}

/** The Standard Schema contract (https://standardschema.dev) route `search` validators follow. */
export interface StandardSchemaV1<Input = unknown, Output = Input> {
  readonly "~standard": {
    readonly version: 1;
    readonly vendor: string;
    readonly validate: (
      value: unknown,
    ) => StandardSchemaResult<Output> | Promise<StandardSchemaResult<Output>>;
    readonly types?: { readonly input: Input; readonly output: Output } | undefined;
  };
}

export type StandardSchemaResult<Output> =
  | { readonly value: Output; readonly issues?: undefined }
  | { readonly issues: ReadonlyArray<{ readonly message: string }> };

export interface Path {
  pathname: string;
  search: string;
  hash: string;
}

export interface Location<S = unknown> extends Path {
  query: SearchParams;
  state: Readonly<Partial<S>> | null;
  key: string;
}

export interface NavigateOptions<S = unknown> {
  /** `true`: resolve like an href against the current route; `false`: `to` is the final path. */
  resolve: boolean;
  replace: boolean;
  scroll: boolean;
  state: S;
}

export interface Navigator {
  (to: string | TypedPath | number, options?: Partial<NavigateOptions>): void;
  (delta: number): void;
}

export interface LocationChange<S = unknown> {
  value: string;
  replace?: boolean;
  scroll?: boolean;
  state?: S;
  rawPath?: string;
}

export type Intent = "initial" | "native" | "navigate" | "preload";

export interface RoutePreloadFuncArgs<P extends Params = Params> {
  params: P;
  location: Location;
  intent: Intent;
}

export type RoutePreloadFunc<T = unknown, P extends Params = Params> = (
  args: RoutePreloadFuncArgs<P>,
) => T;

export interface RouteSectionProps<T = unknown, P extends Params = Params> {
  params: P;
  location: Location;
  data: T;
  children?: JSX.Element;
}

export type RouteSectionComponent<T = unknown, P extends Params = Params> =
  | Component<RouteSectionProps<T, P>>
  | Component<Omit<RouteSectionProps<T, P>, "children">>
  | Component<{}>;

export interface TypedRouteConfig<S extends string = string> {
  readonly [RoutePatternBrand]: S;
}

type RouteDataOf<Path> = Path extends TypedRouteConfig & { preload?: RoutePreloadFunc<infer T> }
  ? T
  : unknown;

/**
 * Route component props typed by a path witness: a `paths` node, a pattern string, or a
 * `defineFileRoute` config (which also types `data` from its `preload`).
 */
export type RouteProps<Path, T = RouteDataOf<Path>> = RouteSectionProps<
  T,
  Path extends TypedPath<infer P>
    ? SimplifyRecord<P> & Params
    : Path extends TypedRouteConfig<infer S>
      ? RouteParams<S>
      : RouteParams<Path>
>;

export type RouteComponent<Path, T = RouteDataOf<Path>> = Component<RouteProps<Path, T>>;

/**
 * A lazy route subtree: `() => import("./feature/routes")`, whose `default` or `routes` export
 * holds the child definitions. Must be deterministic: resolution is cached per thunk and shared
 * by every router instance.
 */
export type LazyRouteChildren = () =>
  | readonly RouteDefinition[]
  | Promise<
      | readonly RouteDefinition[]
      | { default: readonly RouteDefinition[] }
      | { routes: readonly RouteDefinition[] }
    >;

// oxlint-disable-next-line typescript/no-explicit-any -- the default accepts any literal path and preload data
export type RouteDefinition<S extends string | string[] = any, T = any> = {
  path?: S | undefined;
  matchFilters?: MatchFilters<S> | undefined;
  preload?: RoutePreloadFunc<T> | undefined;
  children?: RouteDefinition | readonly RouteDefinition[] | LazyRouteChildren | undefined;
  component?: RouteSectionComponent<T> | undefined;
  // oxlint-disable-next-line typescript/no-explicit-any -- any Standard Schema validator
  search?: StandardSchemaV1<any, any> | undefined;
  info?: RouteInfo | undefined;
};

export type MatchFilter = readonly string[] | RegExp | ((segment: string) => boolean);

export type PathParams<P extends string | readonly string[]> =
  P extends `${infer Head}/${infer Tail}`
    ? [...PathParams<Head>, ...PathParams<Tail>]
    : P extends `:${infer S}?`
      ? [S]
      : P extends `:${infer S}`
        ? [S]
        : P extends `*${infer S}`
          ? [S]
          : [];

// oxlint-disable-next-line typescript/no-explicit-any -- the default accepts any pattern
export type MatchFilters<P extends string | readonly string[] = any> = P extends string
  ? { [K in PathParams<P>[number]]?: MatchFilter }
  : Record<string, MatchFilter>;

export type DefinedRouteFilters<S> = S extends readonly (infer Member extends string)[]
  ? MatchFilters<Member>
  : S extends string | readonly string[]
    ? MatchFilters<S>
    : MatchFilters;

type FilterKeysOf<S> = S extends readonly (infer Member extends string)[]
  ? PathParams<Member>[number]
  : S extends string
    ? PathParams<S>[number]
    : string;

export type ValidFilters<F, S> = {
  [K in keyof F]: string extends K ? F[K] : K extends FilterKeysOf<S> ? MatchFilter : never;
};

type PatternParams<P extends string> = P extends `${infer Head}/${infer Tail}`
  ? PatternParams<Head> & PatternParams<Tail>
  : P extends `:${infer Name}?`
    ? { [K in Name]?: string }
    : P extends `:${infer Name}`
      ? { [K in Name]: string }
      : P extends `*${infer Name}`
        ? { [K in Name]: string }
        : {};

type SimplifyRecord<T> = { [K in keyof T]: T[K] } & {};

/**
 * The params a pattern guarantees (`:id` is `string`, `:tab?` is `string | undefined`, `*rest`
 * is `string`), open to params inherited from parent routes.
 */
export type RouteParams<S> = (S extends readonly (infer Member extends string)[]
  ? SimplifyRecord<PatternParams<Member>>
  : S extends string
    ? string extends S
      ? {}
      : SimplifyRecord<PatternParams<S>>
    : {}) &
  Params;

export interface PathMatch<P extends Params = Params> {
  params: P;
  path: string;
}

export interface RouteMatch extends PathMatch {
  route: RouteDescription;
}

export interface OutputMatch {
  path: string;
  pattern: string;
  match: string;
  params: Params;
  info?: RouteInfo;
}

export interface RouteDescription {
  key: RouteDefinition | LazyBoundary;
  originalPath: string;
  pattern: string;
  component?: RouteSectionComponent;
  preload?: RoutePreloadFunc;
  matcher: (location: string) => PathMatch | null;
  matchFilters?: MatchFilters;
  info?: RouteInfo;
  /** Present on the placeholder standing in for an unresolved lazy subtree. */
  lazy?: LazyBoundary;
}

export interface LazyBoundary {
  thunk: LazyRouteChildren;
  promise?: Promise<readonly RouteDefinition[]>;
  resolved?: readonly RouteDefinition[];
  error?: unknown;
}

export interface Branch {
  routes: RouteDescription[];
  score: number;
  matcher: (location: string) => RouteMatch[] | null;
}

export interface CompiledBranchLevel {
  definition: RouteDefinition;
  originalPath: string;
  pattern: string;
  partial: boolean;
}

export interface CompiledBranch {
  score: number;
  chain: CompiledBranchLevel[];
}

export interface RouteContext {
  parent?: RouteContext;
  pattern: string;
  params: Params;
  path: () => string;
  outlet: () => JSX.Element;
  resolvePath(to: string): string | undefined;
}

export interface BeforeLeaveEventArgs {
  from: Location;
  to: string | number;
  options?: Partial<NavigateOptions>;
  readonly defaultPrevented: boolean;
  preventDefault(): void;
  retry(force?: boolean): void;
}

export interface BeforeLeaveListener {
  listener: (event: BeforeLeaveEventArgs) => void;
  location: Location;
  navigate: Navigator;
}

export interface BeforeLeaveLifecycle {
  subscribe(listener: BeforeLeaveListener): () => void;
  confirm(to: string | number, options?: Partial<NavigateOptions>): boolean;
}

/** Filled by the first `useBeforeLeave`, so apps without leave guards never load them. */
export interface BeforeLeaveSlot {
  current?: BeforeLeaveLifecycle;
}

export interface RouterUtils {
  renderPath(path: string): string;
  parsePath(path: string): string;
  go(delta: number): void;
  beforeLeave: BeforeLeaveSlot;
  paramsWrapper: (getParams: () => Params, branches: () => Branch[]) => Params;
  queryWrapper: (getQuery: () => SearchParams) => SearchParams;
}
export type Submission<T, U> = {
  readonly input: T;
  readonly result?: U;
  // oxlint-disable-next-line typescript/no-explicit-any -- whatever the action threw
  readonly error: any;
  readonly url: string;
  clear(): void;
  /** Re-runs the action with the same input; resolves like the original call. */
  retry(): Promise<U | undefined>;
};

export type NarrowResponse<T> = Exclude<T, Response>;
