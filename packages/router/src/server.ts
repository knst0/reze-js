// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import type { RouterInstance } from "./factory";
import { createBranchesGetter, getRouteMatches, resolveLazySubtree } from "./matching";
import { runWithCacheScope, type QueryCache } from "./query";
import { runPreload, runWithIntent } from "./routing";
import type {
  Branch,
  Location,
  RouteDefinition,
  RoutePreloadFunc,
  RoutePreloadFuncArgs,
  RouteParams,
} from "./types";
import { extractSearchParams, isThenable, mergeParams } from "./utils";

/** What the host hands the collector: the mutation request plus the pre-digested routing halves. */
export interface ServerFunctionOutcome {
  request: Request;
  /** The URL the client shows after the mutation; absent when there is nothing to collect for. */
  targetUrl?: string;
  /** Revalidation keys declared by the mutation; `true` collects everything. */
  revalidateKeys?: string[] | true;
  /** Cookie effects of the mutation, folded into the flight request preloads run under. */
  foldedHeaders?: Headers;
}

export type CollectFlightDataHook = (
  sourceEvent: unknown,
  outcome: ServerFunctionOutcome,
) => Promise<Record<string, unknown> | undefined>;

export interface FlightDataCollectorOptions {
  routes:
    | RouteDefinition
    | readonly RouteDefinition[]
    | (() => RouteDefinition | readonly RouteDefinition[]);
  rootPreload?: RoutePreloadFunc;
  base?: string;
}

function isRouterInstance(
  options: FlightDataCollectorOptions | RouterInstance<readonly RouteDefinition[]>,
): options is RouterInstance<readonly RouteDefinition[]> {
  return typeof options === "function";
}

/**
 * Produces the `collectFlightData` hook for the app's server-function handler. Accepts a
 * `createRouter` instance — its routes, base and root preload are the single source of truth —
 * or an options object for trees not built through the factory.
 *
 * The hook reruns the route data for the URL the client shows after the mutation, collecting
 * each `query` result under its cache key, scoped to the outcome's `revalidateKeys` when the
 * matched routes are unchanged. The payload seeds the client router's cache in one round trip.
 */
export function createFlightDataCollector(
  options: FlightDataCollectorOptions | RouterInstance<readonly RouteDefinition[]>,
): CollectFlightDataHook {
  const {
    routes,
    rootPreload,
    base = "",
  } = isRouterInstance(options)
    ? { routes: options.routes, rootPreload: options.config.preload, base: options.config.base }
    : options;
  if (!routes) throw new Error("createFlightDataCollector requires `routes`");
  const branches = createBranchesGetter(
    routes,
    base,
    isRouterInstance(options) ? options.config.compiled : undefined,
  );
  return async (_sourceEvent, outcome) => {
    const { targetUrl, revalidateKeys } = outcome;
    if (!targetUrl) return undefined;
    const previousUrl = outcome.request.headers.get("referer");
    const cache: QueryCache = new Map();
    try {
      await runWithCacheScope(cache, async () => {
        await resolveLazyMatches(branches, targetUrl, previousUrl);
        runPreloads(branches(), targetUrl, rootPreload);
        const seen = new Set<unknown>();
        for (;;) {
          const pending: Promise<unknown>[] = [];
          for (const entry of cache.values()) {
            const current = entry.current;
            if (isThenable(current) && !seen.has(current)) {
              seen.add(current);
              pending.push(current.catch(() => {}));
            }
          }
          if (!pending.length) break;
          await Promise.all(pending);
        }
      });
    } catch (error: unknown) {
      console.error(error);
    }
    const target = new URL(targetUrl);
    const previous = previousUrl ? new URL(previousUrl, target).pathname : target.pathname;
    const current = getRouteMatches(branches(), target.pathname);
    const before = getRouteMatches(branches(), previous);
    const isNewlyEntered = current.some(
      (match, level) => !before[level] || before[level]!.route.key !== match.route.key,
    );
    const restricted =
      Array.isArray(revalidateKeys) && !isNewlyEntered ? revalidateKeys : undefined;
    let hasKeys = false;
    const payload: Record<string, unknown> = {};
    for (const [key, entry] of cache) {
      if (restricted && !restricted.some((prefix) => prefix && key.startsWith(prefix))) continue;
      const current = entry.current;
      let value = entry.hasSettled
        ? entry.settled
        : isThenable(current)
          ? await current.catch(() => {})
          : current;
      if (value instanceof Response) value = await readFlightBody(value);
      if (value === undefined) continue;
      payload[key] = value;
      hasKeys = true;
    }
    return hasKeys ? payload : undefined;
  };
}

async function resolveLazyMatches(
  branches: () => Branch[],
  targetUrl: string,
  previousUrl: string | null,
): Promise<void> {
  for (;;) {
    const table = branches();
    const target = new URL(targetUrl);
    const previous = previousUrl ? new URL(previousUrl, target).pathname : target.pathname;
    const pending = [
      ...getRouteMatches(table, target.pathname),
      ...getRouteMatches(table, previous),
    ]
      .map((match) => match.route.lazy)
      .filter((boundary) => boundary !== undefined && !boundary.resolved);
    if (!pending.length) return;
    await Promise.all(pending.map((boundary) => resolveLazySubtree(boundary!)));
  }
}

function targetLocation(url: URL): Location {
  return {
    pathname: url.pathname,
    search: url.search,
    hash: url.hash,
    query: extractSearchParams(url),
    state: null,
    key: "",
  };
}

function runPreloads(
  branches: Branch[],
  targetUrl: string,
  rootPreload: RoutePreloadFunc | undefined,
): void {
  const target = new URL(targetUrl);
  const matches = getRouteMatches(branches, target.pathname);
  const location = targetLocation(target);
  if (rootPreload) {
    const args: RoutePreloadFuncArgs<RouteParams<string>> = {
      params: mergeParams(matches) as RouteParams<string>,
      location,
      intent: "initial",
    };
    runWithIntent("initial", () =>
      runPreload(rootPreload as RoutePreloadFunc<unknown>, args as RoutePreloadFuncArgs),
    );
  }
  for (const { route, params } of matches) {
    if (!route.preload) continue;
    const args: RoutePreloadFuncArgs = { params, location, intent: "preload" };
    runWithIntent("preload", () => runPreload(route.preload!, args));
  }
}

async function readFlightBody(response: Response): Promise<unknown> {
  const contentType = response.headers.get("content-type") ?? "";
  if (!contentType.includes("json")) return undefined;
  try {
    const body = (await response.json()) as unknown;
    const envelope = body as { flight?: unknown; data?: unknown };
    return typeof envelope.flight === "object" && envelope.flight !== null ? envelope.data : body;
  } catch {
    return undefined;
  }
}
