// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import type {
  Params,
  RouteDefinition,
  SearchParams,
  SetSearchParams,
  StandardSchemaV1,
  TypedPath,
  TypedSearchPath,
} from "./types";
import { mergeSearchString, normalizePath } from "./utils";

declare const FilterType: unique symbol;

/** A match filter that also types its param's argument in `paths` calls; runtime params stay strings. */
export interface TypedMatchFilter<T> {
  (segment: string): boolean;
  readonly [FilterType]: T;
}

/** Matches integer segments; types the param as `number` in the path proxy. */
export const int = ((segment: string) => /^-?\d+$/.test(segment)) as TypedMatchFilter<number>;

type Flat<T> = { [K in keyof T]: T[K] } & {};

type SegmentsOf<P extends string> = P extends `${infer A}/${infer B}`
  ? [...SegmentsOf<A>, ...SegmentsOf<B>]
  : P extends ""
    ? []
    : [P];

type FilterArg<F> =
  F extends TypedMatchFilter<infer T> ? T : F extends readonly (infer S)[] ? S : string | number;

type ParamArg<Name extends string, F> = Name extends keyof F ? FilterArg<F[Name]> : string | number;

type ParamRun<
  Segs extends readonly string[],
  F,
  Args extends readonly unknown[] = [],
  Acc extends Params = {},
> = Segs extends readonly [infer H extends string, ...infer R extends readonly string[]]
  ? H extends `:${string}?`
    ? { args: Args; params: Acc; rest: Segs }
    : H extends `:${infer N}`
      ? ParamRun<R, F, [...Args, ParamArg<N, F>], Acc & { [K in N]: string }>
      : H extends `*${infer N}`
        ? { args: [...Args, string | number]; params: Acc & { [K in N]: string }; rest: R }
        : { args: Args; params: Acc; rest: Segs }
  : { args: Args; params: Acc; rest: Segs };

export interface SearchTypes {
  input: unknown;
  output: unknown;
}

export interface DefaultSearchTypes {
  input: SetSearchParams;
  output: SearchParams;
}

/** A route end: `()` or `(search, hash?)` produce the href. */
export interface PathEnd<Sch extends SearchTypes = DefaultSearchTypes, P extends Params = Params>
  extends TypedPath<P>, TypedSearchPath<Sch["input"], Sch["output"]> {
  (): string;
  (search: Sch["input"], hash?: string): string;
}

// oxlint-disable-next-line typescript/no-explicit-any -- any Standard Schema validator
type AnySchema = StandardSchemaV1<any, any>;

type SearchTypesOf<Def> = Def extends { search: infer S }
  ? S extends AnySchema
    ? {
        input: NonNullable<S["~standard"]["types"]>["input"];
        output: NonNullable<S["~standard"]["types"]>["output"];
      }
    : DefaultSearchTypes
  : DefaultSearchTypes;

type FiltersOf<Def> = Def extends { matchFilters: infer F } ? F : {};

type ChildrenOf<Def> = Def extends { children: infer C } ? C : undefined;

type ResolvedChildren<C> = C extends () => infer R
  ? Awaited<R> extends infer M
    ? M extends { default: infer D }
      ? D
      : M extends { routes: infer D }
        ? D
        : M
    : never
  : C;

type ChildPaths<C, Acc extends Params> = [C] extends [undefined]
  ? {}
  : ResolvedChildren<C> extends infer RC
    ? RC extends readonly unknown[]
      ? TuplePaths<RC, Acc>
      : RouteContrib<RC, Acc>
    : never;

type TuplePaths<R extends readonly unknown[], Acc extends Params> = R extends readonly [
  infer H,
  ...infer T extends readonly unknown[],
]
  ? RouteContrib<H, Acc> & TuplePaths<T, Acc>
  : {};

type PathLeaf<Sch extends SearchTypes, C, Acc extends Params> = PathEnd<Sch, Flat<Acc>> &
  ChildPaths<C, Acc>;

type PathNode<
  Segs extends readonly string[],
  F,
  Sch extends SearchTypes,
  C,
  Acc extends Params,
> = Segs extends readonly [infer H extends string, ...infer R extends readonly string[]]
  ? H extends `:${infer N}?`
    ? ((arg: ParamArg<N, F>) => PathNode<R, F, Sch, C, Acc & { [K in N]?: string }>) &
        PathNode<R, F, Sch, C, Acc & { [K in N]?: string }>
    : H extends `:${string}` | `*${string}`
      ? ParamCallNode<Segs, F, Sch, C, Acc>
      : { [K in H]: PathNode<R, F, Sch, C, Acc> }
  : PathLeaf<Sch, C, Acc>;

type ParamCallNode<
  Segs extends readonly string[],
  F,
  Sch extends SearchTypes,
  C,
  Acc extends Params,
> =
  ParamRun<Segs, F> extends {
    args: infer A extends readonly unknown[];
    params: infer P2 extends Params;
    rest: infer R2 extends readonly string[];
  }
    ? ((...args: A) => PathNode<R2, F, Sch, C, Acc & P2>) &
        (R2 extends readonly [] ? { (...args: [...A, Sch["input"], string?]): string } : {}) &
        TypedPath<Flat<Acc & P2>>
    : never;

type RouteContrib<Def, Acc extends Params> = Def extends { path: infer P }
  ? [P] extends [undefined]
    ? ChildPaths<ChildrenOf<Def>, Acc>
    : P extends readonly string[]
      ? MultiPathContrib<P, Def, Acc>
      : P extends string
        ? string extends P
          ? UntypedPaths
          : PathNode<SegmentsOf<P>, FiltersOf<Def>, SearchTypesOf<Def>, ChildrenOf<Def>, Acc>
        : UntypedPaths
  : ChildPaths<ChildrenOf<Def>, Acc>;

type MultiPathContrib<Ps extends readonly string[], Def, Acc extends Params> = Ps extends readonly [
  infer H extends string,
  ...infer T extends readonly string[],
]
  ? PathNode<SegmentsOf<H>, FiltersOf<Def>, SearchTypesOf<Def>, ChildrenOf<Def>, Acc> &
      MultiPathContrib<T, Def, Acc>
  : {};

// oxlint-disable-next-line typescript/no-explicit-any -- a route tree built at runtime has no static paths
type UntypedPaths = any;

/** The `paths` proxy type of a literal route tuple; a non-literal tree types as untyped. */
export type RoutePaths<R extends readonly RouteDefinition[]> = number extends R["length"]
  ? UntypedPaths
  : PathEnd<DefaultSearchTypes, {}> & TuplePaths<R, {}>;

export type PathParamsOf<N> = N extends TypedPath<infer P> ? Flat<P> : Params;

/** On a paths node: its logical pathname, before the history adapter's `renderPath`. */
export const HREF: unique symbol = Symbol.for("rezejs.router.href") as typeof HREF;

const encodeParam = (value: unknown): string =>
  String(value).split("/").map(encodeURIComponent).join("/");

export function createPathsProxy(
  renderPath: (path: string) => string = (path) => path,
  base = "",
): UntypedPaths {
  const toHref = (pathname: string, suffix = ""): string => renderPath(pathname || "/") + suffix;
  function node(pathname: string): UntypedPaths {
    const build = (...args: unknown[]): unknown => {
      let path = pathname;
      for (let index = 0; index < args.length; index++) {
        const arg = args[index];
        if (typeof arg === "object" && arg !== null) {
          const hash = typeof args[index + 1] === "string" ? `#${args[index + 1] as string}` : "";
          return toHref(path, mergeSearchString("", arg as SetSearchParams) + hash);
        }
        path += `/${encodeParam(arg)}`;
      }
      return args.length ? node(path) : toHref(path);
    };
    return new Proxy(build, {
      get(_, property) {
        if (property === "toString") return () => toHref(pathname);
        if (typeof property === "symbol") {
          if (property === Symbol.toPrimitive) return () => toHref(pathname);
          return property === HREF ? pathname || "/" : undefined;
        }
        return node(`${pathname}/${property}`);
      },
    });
  }
  return node(normalizePath(base));
}
