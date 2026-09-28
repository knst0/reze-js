import type { Params, RouteComponent, RouteDefinition, PreloadArgs } from "./types";

const Static = 0;
const Param = 1;
const Optional = 2;
const Splat = 3;
type Kind = typeof Static | typeof Param | typeof Optional | typeof Splat;

interface Segment {
  readonly kind: Kind;
  readonly value: string;
}

export interface CompiledRoute {
  readonly def: RouteDefinition;
  /** The joined pattern from the root (`/blog/:id`); `Router.match` reports one per level. */
  readonly pattern: string;
  component: RouteComponent<any, any> | undefined;
  preload: ((args: PreloadArgs<any>) => unknown) | undefined;
  info: Readonly<Record<string, unknown>> | undefined;
  loading: Promise<void> | undefined;
  loadError: unknown;
  isLoaded: boolean;
}

export interface Branch {
  readonly routes: readonly CompiledRoute[];
  readonly segments: readonly Segment[];
}

export interface PathMatch {
  readonly params: Params;
  readonly path: string;
}

export interface BranchMatch extends PathMatch {
  readonly branch: Branch;
}

export const ParamName = /^(?!__proto__$)[A-Za-z_$][\w$]*$/;
export const SplatName = /^(?!__proto__$)[\w$]+$/;

function invalid(path: string, reason: string): Error {
  return new Error(`[reze-router] invalid route path "${path}": ${reason}`);
}

function compileSegments(path: string): Segment[] {
  const segments: Segment[] = [];
  for (const part of path.split("/")) {
    if (part === "") continue;
    const head = part[0];
    if (head === ":") {
      const isOptional = part.endsWith("?");
      const name = part.slice(1, isOptional ? -1 : undefined);
      if (!ParamName.test(name)) throw invalid(path, `bad param name "${name}"`);
      segments.push({ kind: isOptional ? Optional : Param, value: name });
    } else if (head === "*") {
      const name = part.length === 1 ? "*" : part.slice(1);
      if (name !== "*" && !SplatName.test(name)) throw invalid(path, `bad splat name "${name}"`);
      segments.push({ kind: Splat, value: name });
    } else {
      segments.push({ kind: Static, value: decode(part).toLowerCase() });
    }
  }
  for (let i = 0; i < segments.length; i++) {
    const kind = segments[i]!.kind;
    if (kind === Splat && i !== segments.length - 1) throw invalid(path, "a splat must be the last segment");
    if (kind === Optional) {
      for (let j = i + 1; j < segments.length; j++) {
        const later = segments[j]!.kind;
        if (later !== Optional && later !== Splat) throw invalid(path, "an optional param must not precede a required segment");
      }
    }
  }
  return segments;
}

function requiredLength(segments: readonly Segment[]): number {
  let length = segments.length;
  while (length > 0 && segments[length - 1]!.kind === Optional) length--;
  return length;
}

function joinPaths(parent: string, child: string): string {
  return parent.replace(/\/+$/, "") + "/" + child.replace(/^\/+/, "");
}

/** Rank of `segments[i]`, higher first: static, then param, then the path's end, then splat. */
function rankAt(segments: readonly Segment[], i: number): number {
  if (i >= segments.length) return 1;
  const kind = segments[i]!.kind;
  return kind === Static ? 3 : kind === Splat ? 0 : 2;
}

function compareBranches(a: Branch, b: Branch): number {
  const length = Math.max(a.segments.length, b.segments.length);
  for (let i = 0; i < length; i++) {
    const difference = rankAt(b.segments, i) - rankAt(a.segments, i);
    if (difference !== 0) return difference;
  }
  return 0;
}

function compileRoute(def: RouteDefinition, pattern: string): CompiledRoute {
  return {
    def,
    pattern,
    component: def.component,
    preload: def.preload,
    info: def.info,
    loading: undefined,
    loadError: undefined,
    isLoaded: def.load === undefined,
  };
}

/** Flattens `defs` into leaf branches, best match first: segment by segment, static beats param beats splat; ties keep definition order. Throws on an invalid path. */
export function compileRoutes(defs: readonly RouteDefinition[]): Branch[] {
  const branches: Branch[] = [];
  const walk = (list: readonly RouteDefinition[], parentPath: string, parents: readonly CompiledRoute[]): void => {
    for (const def of list) {
      const fullPath = joinPaths(parentPath, def.path);
      const routes = [...parents, compileRoute(def, fullPath)];
      const segments = compileSegments(fullPath);
      if (def.children !== undefined && def.children.length > 0) {
        walk(def.children, fullPath, routes);
        continue;
      }
      for (let length = requiredLength(segments); length <= segments.length; length++) {
        const expanded: Segment[] = segments
          .slice(0, length)
          .map((s) => (s.kind === Optional ? { kind: Param, value: s.value } : s) satisfies Segment);
        branches.push({ routes, segments: expanded });
      }
    }
  };
  walk(defs, "", []);
  return branches.sort(compareBranches);
}

/** `decodeURIComponent`, keeping `value` as is when it is malformed. */
export function decode(value: string): string {
  if (!value.includes("%")) return value;
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

/** `parts` are decoded path segments; static segments compare case-insensitively. */
function matchSegments(segments: readonly Segment[], parts: readonly string[], isPrefix: boolean): Record<string, string> | undefined {
  const params: Record<string, string> = {};
  for (let i = 0; i < segments.length; i++) {
    const segment = segments[i]!;
    if (segment.kind === Splat) {
      params[segment.value] = parts.slice(i).join("/");
      return params;
    }
    const part = parts[i];
    if (part === undefined) return undefined;
    if (segment.kind === Static) {
      if (part.toLowerCase() !== segment.value) return undefined;
    } else {
      params[segment.value] = part;
    }
  }
  return isPrefix || parts.length === segments.length ? params : undefined;
}

function splitPath(pathname: string): string[] {
  return pathname.split("/").filter((part) => part !== "");
}

/** `pathname` compared as `matchBranches` does: lowercased, empty segments dropped; `/` for the root. */
export function pathKey(pathname: string): string {
  return "/" + splitPath(pathname.toLowerCase()).join("/");
}

/** The first of `branches` matching `pathname`, with decoded params and the normalised matched path. */
export function matchBranches(branches: readonly Branch[], pathname: string): BranchMatch | undefined {
  const raw = splitPath(pathname);
  const parts = raw.map(decode);
  for (const branch of branches) {
    const params = matchSegments(branch.segments, parts, false);
    if (params !== undefined) return { branch, params, path: "/" + raw.join("/") };
  }
  return undefined;
}

/** Matches one route pattern; a trailing bare `/*` turns it into a prefix match. Throws on an invalid pattern. */
export function matchPath(pattern: string, pathname: string): PathMatch | undefined {
  const segments = compileSegments(pattern);
  const last = segments[segments.length - 1];
  const isPrefix = last !== undefined && last.kind === Splat && last.value === "*";
  const raw = splitPath(pathname);
  const parts = raw.map(decode);
  const required = requiredLength(segments);
  for (let length = segments.length; length >= required; length--) {
    const params = matchSegments(isPrefix ? segments.slice(0, -1) : segments.slice(0, length), parts, isPrefix);
    if (params !== undefined) {
      const matched = isPrefix ? raw.slice(0, segments.length - 1) : raw;
      return { params, path: "/" + matched.join("/") };
    }
    if (isPrefix) break;
  }
  return undefined;
}
