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

export interface RouteProps<P extends Params = Params, D = unknown> {
  readonly params: P;
  readonly location: Location;
  readonly data: D;
  readonly children: JSX.Element;
}

export type RouteComponent<P extends Params = Params, D = unknown> = (props: RouteProps<P, D>) => JSX.Element;

export interface RouteConfig<D = unknown> {
  /**
   * Starts the route's data; the return value is the component's `data`. Runs with intent `"preload"` when a link to the
   * route is hovered, focused or touched, and again with `"navigate"` when it is entered, so cache fetches that should not repeat.
   */
  preload?: (args: PreloadArgs<any>) => D;
  info?: Readonly<Record<string, unknown>>;
}

export interface RouteModule {
  default?: RouteComponent<any, any>;
  route?: RouteConfig<any>;
}

export interface RouteDefinition extends RouteConfig<any> {
  path: string;
  component?: RouteComponent<any, any>;
  /** Code-split module; its `default` and `route` override `component`, `preload` and `info` once loaded. */
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

/** Augmented by the generated routes `.d.ts` with `paths`, every route path, and `base`, the prefix `<a href>` puts before one: Vite's base without its trailing slash, or `#` for hash history. */
export interface Register {}

export type RoutePath = Register extends { paths: infer P extends string } ? P : string;

type Base = Register extends { base: infer B extends string } ? B : "";

type ForeignHref = `${string}:${string}` | `//${string}` | `#${string}` | `?${string}`;

type Target<Prefix extends string, Path extends string> = `${Prefix}${Path}` | `${Prefix}${Path}?${string}` | `${Prefix}${Path}#${string}` | ForeignHref;

/** A `navigate()` target: a route path (without `base`), optionally with a query or hash, or an absolute URL. */
export type NavigateTarget = string extends RoutePath ? string : Target<"", RoutePath>;

/**
 * An `<a href>`: a route path under `base`, optionally with a query or hash, or an absolute URL. A dynamic segment accepts any
 * text, slashes included, so a route with a dynamic first segment (`[id].tsx`) makes every root-relative href valid.
 */
export type Href = string extends RoutePath ? string : Target<Base, RoutePath>;
