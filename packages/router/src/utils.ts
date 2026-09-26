// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { computed, getOwner, runWithOwner } from "@rezejs/signals";

import type {
  LocationChange,
  MatchFilter,
  MatchFilters,
  Params,
  PathMatch,
  SearchParams,
  SetSearchParams,
  StandardSchemaResult,
  StandardSchemaV1,
} from "./types";

const hasSchemeRegex = /^(?:[a-z0-9]+:)?\/\//i;
const trimPathRegex = /^\/+|(\/)\/+$/g;

export const mockBase = "http://sr";

export function normalizePath(path: string, omitSlash = false): string {
  const trimmed = path.replace(trimPathRegex, "$1");
  if (!trimmed) return "";
  return omitSlash || /^[?#]/.test(trimmed) ? trimmed : "/" + trimmed;
}

export function comparablePath(path: string): string {
  return normalizePath(path.split(/[?#]/, 1)[0]!).toLowerCase().replace(/\/$/, "");
}

export function resolvePath(base: string, path: string, from?: string): string | undefined {
  if (hasSchemeRegex.test(path)) return undefined;
  const basePath = normalizePath(base);
  const fromPath = from && normalizePath(from);
  let result: string;
  if (!fromPath || path.startsWith("/")) {
    result = basePath;
  } else if (fromPath.toLowerCase().indexOf(basePath.toLowerCase()) !== 0) {
    result = basePath + fromPath;
  } else {
    result = fromPath;
  }
  return (result || "/") + normalizePath(path, !result);
}

export function joinPaths(from: string, to: string): string {
  return normalizePath(from).replace(/\/*(\*.*)?$/g, "") + normalizePath(to);
}

export function validateSearch<Output>(
  schema: StandardSchemaV1<unknown, Output>,
  raw: SearchParams,
): StandardSchemaResult<Output> {
  const outcome = schema["~standard"].validate(raw);
  if (outcome instanceof Promise) {
    throw new Error("Async Standard Schema validation is not supported for search params");
  }
  return outcome;
}

export function extractSearchParams(url: URL): SearchParams {
  const params: Record<string, string | string[]> = {};
  url.searchParams.forEach((value, key) => {
    const existing = params[key];
    if (existing === undefined) params[key] = value;
    else if (Array.isArray(existing)) existing.push(value);
    else params[key] = [existing, value];
  });
  return params;
}

export function createMatcher(
  path: string,
  isPartial?: boolean,
  matchFilters?: MatchFilters,
): (location: string) => PathMatch | null {
  const [pattern, splat] = path.split("/*", 2) as [string, string | undefined];
  const segments = pattern.split("/").filter(Boolean);
  const length = segments.length;
  return (location) => {
    const locationSegments = location.split("/");
    if (locationSegments[0] === "") locationSegments.shift();
    if (locationSegments.length && locationSegments[locationSegments.length - 1] === "") {
      locationSegments.pop();
    }
    if (locationSegments.includes("")) return null;
    const extraLength = locationSegments.length - length;
    if (extraLength < 0 || (extraLength > 0 && splat === undefined && !isPartial)) return null;
    const match: PathMatch = { path: length ? "" : "/", params: {} };
    for (let index = 0; index < length; index++) {
      const segment = segments[index]!;
      const isDynamic = segment[0] === ":";
      const locationSegment = isDynamic
        ? locationSegments[index]!
        : locationSegments[index]!.toLowerCase();
      const key = isDynamic ? segment.slice(1) : segment.toLowerCase();
      if (isDynamic && matchSegment(locationSegment, matchFilters?.[key])) {
        match.params[key] = locationSegment;
      } else if (isDynamic || !matchSegment(locationSegment, key)) {
        return null;
      }
      match.path += `/${locationSegment}`;
    }
    if (splat) {
      const remainder = extraLength ? locationSegments.slice(-extraLength).join("/") : "";
      if (!matchSegment(remainder, matchFilters?.[splat])) return null;
      match.params[splat] = remainder;
    }
    return match;
  };
}

function matchSegment(input: string, filter: MatchFilter | string | undefined): boolean {
  if (filter === undefined) return true;
  if (typeof filter === "string") return filter === input;
  if (typeof filter === "function") return filter(input);
  if (filter instanceof RegExp) return filter.test(input);
  return filter.includes(input);
}

export function scoreRoute(pattern: string): number {
  const [head, splat] = pattern.split("/*", 2) as [string, string | undefined];
  const segments = head.split("/").filter(Boolean);
  return segments.reduce(
    (score, segment) => score + (segment.startsWith(":") ? 2 : 3),
    segments.length - (splat === undefined ? 0 : 1),
  );
}

/** A proxy reading `read()[key]`, each key through its own computed so readers of one key skip changes to the others. */
export function createMemoObject<T extends Record<string, unknown>>(read: () => T): T {
  const memos = new Map<PropertyKey, () => unknown>();
  const owner = getOwner();
  return new Proxy({} as T, {
    get(_, property) {
      let memo = memos.get(property);
      if (memo === undefined) {
        memo = runWithOwner(owner, () =>
          computed(() => (read() as Record<PropertyKey, unknown>)[property]),
        );
        memos.set(property, memo);
      }
      return memo();
    },
    getOwnPropertyDescriptor(_, property) {
      if (!(property in read())) return undefined;
      return {
        enumerable: true,
        configurable: true,
        value: (read() as Record<PropertyKey, unknown>)[property],
      };
    },
    ownKeys() {
      return Reflect.ownKeys(read());
    },
    has(_, property) {
      return property in read();
    },
  });
}

export function mergeSearchString(search: string, params: SetSearchParams): string {
  const merged = new URLSearchParams(search);
  for (const [key, value] of Object.entries(params)) {
    if (value == null || value === "" || (Array.isArray(value) && !value.length)) {
      merged.delete(key);
    } else if (Array.isArray(value)) {
      merged.delete(key);
      for (const item of value) merged.append(key, String(item));
    } else {
      merged.set(key, String(value));
    }
  }
  const serialized = merged.toString();
  return serialized ? `?${serialized}` : "";
}

/** `/:a?/:b?` expands to `/`, `/:a`, `/:a/:b`: adjacent optionals only grow in order. */
export function expandOptionals(pattern: string): string[] {
  let match = /(\/?:[^/]+)\?/.exec(pattern);
  if (!match) return [pattern];
  let prefix = pattern.slice(0, match.index);
  let suffix = pattern.slice(match.index + match[0].length);
  const prefixes = [prefix, (prefix += match[1])];
  while ((match = /^(\/:[^/]+)\?/.exec(suffix))) {
    prefixes.push((prefix += match[1]));
    suffix = suffix.slice(match[0].length);
  }
  return expandOptionals(suffix).reduce<string[]>(
    (results, expansion) => [...results, ...prefixes.map((head) => head + expansion)],
    [],
  );
}

export function mergeParams(matches: readonly { params: Params }[]): Params {
  const params: Params = {};
  for (const match of matches) Object.assign(params, match.params);
  return params;
}
export function isThenable(value: unknown): value is Promise<unknown> {
  return (
    (typeof value === "object" || typeof value === "function") &&
    value !== null &&
    typeof (value as { then?: unknown }).then === "function"
  );
}

export function isSameLocationChange(a: LocationChange, b: LocationChange): boolean {
  return a.value === b.value && a.state === b.state;
}
