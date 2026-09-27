import type { JSX } from "@rezejs/dom";

export type Params = Readonly<Record<string, string>>;

export interface Location {
  readonly pathname: string;
  readonly search: string;
  readonly hash: string;
  readonly query: Readonly<Record<string, string>>;
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

/** Navigates to `to`, resolved against the current location; a number moves through history. */
export type Navigate = (to: Href | number, options?: NavigateOptions) => void;

export interface BeforeLeaveEvent {
  readonly from: Location;
  readonly to: string | number;
  readonly options: NavigateOptions;
  readonly defaultPrevented: boolean;
  preventDefault(): void;
  /** Re-runs the prevented navigation; `force` (default `true`) skips the leave guards. */
  retry(force?: boolean): void;
}

/** Augmented by the generated routes `.d.ts` with `paths`, the union of every route href. */
export interface Register {}

export type RoutePath = Register extends { paths: infer P extends string } ? P : string;

export type Href = string extends RoutePath
  ? string
  : RoutePath | `${RoutePath}?${string}` | `${RoutePath}#${string}` | `${string}:${string}` | `//${string}` | `#${string}` | `?${string}`;
