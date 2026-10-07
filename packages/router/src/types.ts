import type { JSX } from "@rezejs/dom";

export type Params = Readonly<Record<string, string>>;

export interface Location {
  readonly pathname: string;
  readonly search: string;
  readonly hash: string;
  /** Repeated keys (`?tag=a&tag=b`) map to every value in order. */
  readonly query: Readonly<Record<string, string | readonly string[]>>;
  readonly state: unknown;
}

export type PreloadIntent = "initial" | "navigate" | "preload";

export interface PreloadArgs<P extends Params = Params> {
  readonly params: P;
  readonly location: Location;
  readonly intent: PreloadIntent;
}

export interface RouteResolvedArgs<P extends Params = Params, D = unknown> extends PreloadArgs<P> {
  readonly data: Awaited<D>;
}

export interface RouteProps<P extends Params = Params, D = unknown> {
  readonly params: P;
  readonly location: Location;
  readonly data: D;
  readonly children: JSX.Element;
}

export type RouteComponent<P extends Params = Params, D = unknown> = (props: RouteProps<P, D>) => JSX.Element;

export type Awaitable<T> = T | PromiseLike<T>;

export interface PageMetadata {
  title?: string;
  description?: string;
  canonical?: string;
  robots?: string;
}

export interface RouteRedirect {
  to: string;
  replace?: boolean;
}

export interface RouteConfig<D = unknown> {
  /**
   * Starts the route's data; the return value is the component's `data`. Runs with intent `"preload"` when a link to the
   * route is hovered, focused or touched, and again with `"navigate"` when it is entered, so cache fetches that should not repeat.
   * May return a promise-like; navigations await it and deliver the settled value, never the promise.
   */
  preload?: (args: PreloadArgs<any>) => D;
  /**
   * Page metadata, merged root-to-leaf with the last defined field winning. A function receives the route's settled
   * preload data; navigations await it. Hover warming never runs it.
   */
  meta?: PageMetadata | ((args: RouteResolvedArgs<any, D>) => Awaitable<PageMetadata>);
  /**
   * A literal object short-circuits the navigation before any preload runs. A function receives the route's settled
   * preload data; navigations await it and follow the first defined result root-to-leaf. Hover warming never runs it.
   */
  redirect?: RouteRedirect | ((args: RouteResolvedArgs<any, D>) => Awaitable<RouteRedirect | undefined>);
  info?: Readonly<Record<string, unknown>>;
}

export interface RouteModule {
  default?: RouteComponent<any, any>;
  route?: RouteConfig<any>;
}

export interface RouteDefinition extends RouteConfig<any> {
  path: string;
  /** Stable identity for hydration seeds and SSG descriptors: the plugin sets it to the file route id, hand-written tables may. Otherwise the factory assigns a structural index chain (`"2"`, `"2/0"`). */
  id?: string;
  /** Node key in `Router.paths`, from the file segment (`blog/[id]` → `byId`) or derived from `path`; the plugin sets it, hand-written tables may. Siblings sharing a key throw at factory creation. */
  name?: string;
  component?: RouteComponent<any, any>;
  /** Code-split module; its `default` and `route` override `component`, `preload`, `meta`, `redirect` and `info` once loaded. */
  load?: () => Promise<RouteModule>;
  children?: readonly RouteDefinition[];
}

export interface RouteMatch {
  readonly path: string;
  readonly params: Params;
  readonly data: unknown;
  readonly info: Readonly<Record<string, unknown>> | undefined;
}

export interface NavigateOptions {
  replace?: boolean;
  state?: unknown;
  /** Default `true`: scroll to the hash target or the top after navigating. */
  scroll?: boolean;
}

/** Navigates to `to`: a route path resolved against the current location, an absolute URL (loaded by the browser when outside the router), or a history delta. */
export type Navigate = (to: NavigateTarget | number, options?: NavigateOptions) => void;

export interface BeforeLeaveEvent {
  readonly from: Location;
  /** The target path, a history delta, or `null` when the document itself is being unloaded. */
  readonly to: string | number | null;
  readonly options: NavigateOptions;
  readonly defaultPrevented: boolean;
  /** Blocks the navigation; for a document unload the browser asks the user to confirm. */
  preventDefault(): void;
  /** Re-runs the prevented navigation; `force` (default `true`) skips the leave guards. No-op for a document unload. */
  retry(force?: boolean): void;
}

/** Augmented by the generated routes `.d.ts`: `paths` (every leaf href shape) and `base` (the prefix `<a href>` puts before one, or `#` for hash history) drive `Href`; `routes` (leaf pattern to params and data) and `pathsTree` (the `paths` builders) drive the typed hooks and `Router.paths`. */
export interface Register {}

export type RoutePath = Register extends { paths: infer P extends string } ? P : string;

type Base = Register extends { base: infer B extends string } ? B : "";

type ForeignHref = `${string}:${string}` | `//${string}` | `#${string}` | `?${string}`;

type Target<Prefix extends string, Path extends string> =
  | `${Prefix}${Path}`
  | `${Prefix}${Path}?${string}`
  | `${Prefix}${Path}#${string}`
  | ForeignHref;

/** A `navigate()` target: a route path (without `base`), optionally with a query or hash, or an absolute URL. */
export type NavigateTarget = string extends RoutePath ? string : Target<"", RoutePath>;

/**
 * An `<a href>`: a route path under `base`, optionally with a query or hash, or an absolute URL. A dynamic segment accepts any
 * text, slashes included, so a route with a dynamic first segment (`[id].tsx`) makes every root-relative href valid.
 */
export type Href = string extends RoutePath ? string : Target<Base, RoutePath>;

/** Href shapes of one concrete route shape `H`: itself, with a query, with a hash. */
export type HrefFor<H extends string> = H | `${H}?${string}` | `${H}#${string}`;

/** A route module's data: the awaited return of its `route.preload`, or `unknown` without one. */
export type DataOf<M> = M extends { route: { preload: (...args: never[]) => infer D } } ? Awaited<D> : unknown;

/** Every leaf pattern (`/blog/:id`); `string` without the plugin. */
export type RoutePattern = Register extends { routes: infer R } ? keyof R & string : string;

/** The merged params of leaf `P`; `Params` without the plugin. */
export type ParamsFor<P extends RoutePattern> = Register extends { routes: infer R }
  ? P extends keyof R
    ? R[P] extends { params: infer Q extends Record<string, string | undefined> }
      ? Q
      : Params
    : Params
  : Params;

/** The preload data of leaf `P`; `unknown` without the plugin. */
export type DataFor<P extends RoutePattern> = Register extends { routes: infer R }
  ? P extends keyof R
    ? R[P] extends { data: infer D }
      ? D
      : unknown
    : unknown
  : unknown;

/** Component props for leaf `P`: params and data follow the route, no manual generics. */
export type RoutePropsFor<P extends RoutePattern> = RouteProps<ParamsFor<P>, DataFor<P>>;

/**
 * A file route's `route` export for leaf `P`, written `export const route = { … } satisfies RouteConfigFor<P, D>`:
 * preload params follow the pattern, the return stays author-declared, and `D` defaults to `unknown` for modules
 * without data.
 */
export interface RouteConfigFor<P extends RoutePattern, D = unknown> {
  preload?: (args: PreloadArgs<ParamsFor<P>>) => D;
  meta?: PageMetadata | ((args: RouteResolvedArgs<ParamsFor<P>, D>) => Awaitable<PageMetadata>);
  redirect?: RouteRedirect | ((args: RouteResolvedArgs<ParamsFor<P>, D>) => Awaitable<RouteRedirect | undefined>);
  info?: Readonly<Record<string, unknown>>;
}

type SegmentParam<S extends string> = [S] extends ["*"]
  ? { readonly "*": string }
  : [S] extends [`*${infer N}`]
    ? { readonly [K in N]: string }
    : [S] extends [`:${infer N}`]
      ? { readonly [K in N extends `${infer M}?` ? M : N]: string }
      : {};

/** Params parsed from a pattern string; optional segments type as `string`, like `Params`. */
export type RouteParams<S extends string> = [S] extends [`${infer Head}/${infer Tail}`]
  ? SegmentParam<Head> & RouteParams<Tail>
  : SegmentParam<S>;

/** One root-to-leaf hit of `Router.match`, without rendering. */
export interface OutputMatch {
  readonly path: string;
  readonly pattern: string;
  readonly params: Params;
  readonly info: Readonly<Record<string, unknown>> | undefined;
}

/** Query accepted by the `paths` builders and `useSearchParams`: `null`/`undefined` delete a key, an array appends one entry per item. */
export type SearchValue = string | number | boolean | null | undefined;
export type SearchInit = Record<string, SearchValue | readonly SearchValue[]>;

/** Loose builder tree for hand-written routes; the plugin emits the precise one as `Register.pathsTree`. */
export interface LoosePathNode {
  (...args: readonly unknown[]): string & LoosePathNode;
  [key: string]: LoosePathNode;
}
export interface LoosePaths {
  [key: string]: LoosePathNode;
}

/** The `paths` builders: generated per file route, loose otherwise. */
export type PathsTree = Register extends { pathsTree: infer T } ? T : LoosePaths;
