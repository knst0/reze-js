// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { signal } from "@rezejs/signals";

import type {
  Branch,
  LazyBoundary,
  LazyRouteChildren,
  RouteDefinition,
  RouteDescription,
  RouteMatch,
} from "./types";
import { createMatcher, expandOptionals, joinPaths, scoreRoute } from "./utils";

const lazyBoundaries = new WeakMap<LazyRouteChildren, LazyBoundary>();

const lazyTreeVersion = /* @__PURE__ */ signal(0);

/** The lazy-subtree version, tracked: compiled branches recompile when a subtree resolves or fails. */
export function trackLazySubtrees(): number {
  return lazyTreeVersion[0]();
}

function lazyBoundaryOf(thunk: LazyRouteChildren): LazyBoundary {
  let boundary = lazyBoundaries.get(thunk);
  if (boundary === undefined) {
    boundary = { thunk };
    lazyBoundaries.set(thunk, boundary);
  }
  return boundary;
}

/**
 * Starts (or joins) loading a lazy subtree. A failure is kept on the boundary, where the
 * placeholder route throws it, until the next call retries the import.
 */
export function resolveLazySubtree(boundary: LazyBoundary): Promise<readonly RouteDefinition[]> {
  if (boundary.resolved) return Promise.resolve(boundary.resolved);
  boundary.error = undefined;
  return (boundary.promise ??= Promise.resolve(boundary.thunk()).then(
    (module) => {
      const routes =
        "default" in module ? module.default : "routes" in module ? module.routes : module;
      boundary.resolved = routes;
      lazyTreeVersion[1]((version) => version + 1);
      return routes;
    },
    (error: unknown) => {
      boundary.promise = undefined;
      boundary.error = error ?? new Error("lazy route subtree failed to load");
      lazyTreeVersion[1]((version) => version + 1);
      throw error;
    },
  ));
}

export function unresolvedLazyMatches(matches: readonly RouteMatch[]): LazyBoundary[] {
  const pending: LazyBoundary[] = [];
  for (const match of matches) {
    const boundary = match.route.lazy;
    if (boundary !== undefined && !boundary.resolved) pending.push(boundary);
  }
  return pending;
}

function createLazyPlaceholder(pattern: string, boundary: LazyBoundary): RouteDescription {
  const placeholderPattern = pattern + "/*";
  return {
    key: boundary,
    originalPath: "*",
    pattern: placeholderPattern,
    matcher: createMatcher(placeholderPattern),
    lazy: boundary,
  };
}

const encodeSegment = (segment: string): string =>
  encodeURIComponent(segment).replace(/%(2B|40|3A|24|26|2C|3B|3D)/g, (escape) =>
    decodeURIComponent(escape),
  );

function asArray<T>(value: T | readonly T[]): readonly T[] {
  return Array.isArray(value) ? (value as readonly T[]) : [value as T];
}

function createRoutes(definition: RouteDefinition, base = ""): RouteDescription[] {
  const { component, preload, children, info } = definition;
  const isLeaf = !children || (Array.isArray(children) && !children.length);
  const routes: RouteDescription[] = [];
  for (const originalPath of asArray<string>(definition.path ?? "")) {
    for (const expandedPath of expandOptionals(originalPath)) {
      const path = joinPaths(base, expandedPath);
      const pattern = (isLeaf ? path : path.split("/*", 1)[0]!)
        .split("/")
        .map((segment) =>
          segment.startsWith(":") || segment.startsWith("*") ? segment : encodeSegment(segment),
        )
        .join("/");
      routes.push({
        key: definition,
        component,
        preload,
        info,
        originalPath,
        pattern,
        matcher: createMatcher(pattern, !isLeaf, definition.matchFilters),
      });
    }
  }
  return routes;
}

function createBranch(routes: RouteDescription[], index: number): Branch {
  return {
    routes,
    score: scoreRoute(routes[routes.length - 1]!.pattern) * 10000 - index,
    matcher(location) {
      const matches: RouteMatch[] = [];
      for (let level = routes.length - 1; level >= 0; level--) {
        const route = routes[level]!;
        const match = route.matcher(location);
        if (!match) return null;
        matches.unshift({ ...match, route });
      }
      return matches;
    },
  };
}

/** Every root-to-leaf chain of `definitions`, most specific first. */
export function createBranches(
  definitions: RouteDefinition | readonly RouteDefinition[],
  base = "",
): Branch[] {
  const branches: Branch[] = [];
  collectBranches(definitions, base, [], branches);
  return branches.sort((a, b) => b.score - a.score);
}

function collectBranches(
  definitions: RouteDefinition | readonly RouteDefinition[],
  base: string,
  stack: RouteDescription[],
  branches: Branch[],
): void {
  for (const definition of asArray(definitions)) {
    if (!definition || typeof definition !== "object") continue;
    for (const route of createRoutes(definition, base)) {
      stack.push(route);
      let children = definition.children;
      if (typeof children === "function") {
        const boundary = lazyBoundaryOf(children);
        if (boundary.resolved) {
          children = boundary.resolved;
        } else {
          stack.push(createLazyPlaceholder(route.pattern, boundary));
          branches.push(createBranch([...stack], branches.length));
          stack.pop();
          stack.pop();
          continue;
        }
      }
      if (children && !(Array.isArray(children) && children.length === 0)) {
        collectBranches(children, route.pattern, stack, branches);
      } else {
        branches.push(createBranch([...stack], branches.length));
      }
      stack.pop();
    }
  }
}

export function getRouteMatches(branches: readonly Branch[], location: string): RouteMatch[] {
  for (const branch of branches) {
    const match = branch.matcher(location);
    if (match) return match;
  }
  return [];
}
