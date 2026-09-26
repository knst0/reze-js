// Ported from @solidjs/router (MIT, Copyright (c) 2020-2022 Ryan Carniato).
import { isServerRender } from "@rezejs/dom";
import { getOwner, onCleanup, signal, untrack, useContext } from "@rezejs/signals";

import { getInPreloadFn, getIntent, RouterContext } from "./routing";
import type { Intent } from "./types";
import { isThenable } from "./utils";

const PreloadWindowMs = 5000;
const CacheSweepMs = 180000;
const SweepIntervalMs = 300000;

export const RevalidateHeader = "X-Revalidate";
const RevalidateAll = "*";
export const SingleFlightHeader = "X-Single-Flight";
const LocationHeader = "Location";

export interface CacheEntry {
  stamp: number;
  current: unknown;
  settled: unknown;
  hasSettled: boolean;
  intent: Intent | undefined;
  version: () => number;
  bump: (next: number | ((previous: number) => number)) => number;
  refs: number;
}

export type QueryCache = Map<string, CacheEntry>;

const clientCache: QueryCache = new Map();

let sweepTimer = 0;

function scheduleSweep(): void {
  if (sweepTimer || isServerRender()) return;
  sweepTimer = window.setInterval(() => {
    const now = Date.now();
    for (const [key, entry] of clientCache) {
      if (!entry.refs && now - entry.stamp > CacheSweepMs) clientCache.delete(key);
    }
  }, SweepIntervalMs);
}

let collectionCache: QueryCache | undefined;

/** Runs `fn` with `query` calls landing in `cache`; the scope covers `fn` until it settles. */
export async function runWithCacheScope<T>(cache: QueryCache, fn: () => T): Promise<Awaited<T>> {
  const previous = collectionCache;
  collectionCache = cache;
  try {
    return (await fn()) as Awaited<T>;
  } finally {
    collectionCache = previous;
  }
}

function getCache(): QueryCache {
  if (collectionCache) return collectionCache;
  const owner = getOwner();
  const router = owner ? useContext(RouterContext) : undefined;
  if (router?.queryCache) return router.queryCache as QueryCache;
  if (isServerRender()) {
    throw new Error("query() on the server needs a router or a collection scope");
  }
  scheduleSweep();
  return clientCache;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== "object") return false;
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

export function hashKey(args: readonly unknown[]): string {
  return JSON.stringify(args, (_, value: unknown) =>
    isPlainObject(value)
      ? Object.keys(value)
          .sort()
          .reduce<Record<string, unknown>>((sorted, key) => {
            sorted[key] = (value as Record<string, unknown>)[key];
            return sorted;
          }, {})
      : (value as unknown),
  );
}

export function matchKey(key: string, keys: readonly string[]): boolean {
  for (const prefix of keys) if (prefix && key.startsWith(prefix)) return true;
  return false;
}

/** Reads an `X-Revalidate` value into prefix keys; `*` reads as `undefined` (everything). */
export function readRevalidateKeys(value: string): string[] | undefined {
  const keys = value.split(",");
  return keys.includes(RevalidateAll) ? undefined : keys;
}

export function cacheKeyOp(
  key: string | string[] | void,
  apply: (entry: CacheEntry) => void,
): void {
  const keys = key === undefined ? undefined : Array.isArray(key) ? key : [key];
  for (const [cachedKey, entry] of clientCache) {
    if (keys === undefined || matchKey(cachedKey, keys)) apply(entry);
  }
}

/** Retriggers live matching entries. Keys match by prefix; omitted keys match everything. */
export function revalidate(key?: string | string[] | void, force = true): void {
  const now = Date.now();
  cacheKeyOp(key, (entry) => {
    if (force) entry.stamp = 0;
    entry.bump((version) => (version === now ? now + 1 : now));
  });
}

function isResponse(value: unknown): value is Response {
  return value instanceof Response;
}

export type QueryFunction<T extends (...args: never[]) => unknown> = ((
  ...args: Parameters<T>
) => ReturnType<T>) & {
  key: string;
  keyFor(...args: Parameters<T>): string;
};

/**
 * Declares a cached function: reads dedupe by `name` plus a stable serialization of the
 * arguments, retrigger through `revalidate`. Works without a router; inside one, redirects and
 * revalidation headers of `Response` results navigate and invalidate.
 */
export function query<T extends (...args: never[]) => unknown>(
  fn: T,
  name: string,
): QueryFunction<T> {
  const cachedFn = (...args: Parameters<T>): ReturnType<T> => {
    const cache = getCache();
    const server = isServerRender();
    const intent = untrack(getIntent);
    const inPreloadFn = getInPreloadFn();
    const owner = getOwner();
    const router = owner && !server ? useContext(RouterContext) : undefined;
    const navigate = router?.navigatorFactory();
    const now = Date.now();
    const key = name + hashKey(args);
    let cached = cache.get(key);
    const track = (): void => {
      if (!cached || server || !getOwner()) return;
      cached.refs++;
      cached.version();
      onCleanup(() => void cached!.refs--);
    };
    if (
      cached &&
      cached.stamp &&
      (server || intent === "native" || cached.refs || now - cached.stamp < PreloadWindowMs)
    ) {
      track();
      if (cached.intent === "preload" && intent !== "preload") cached.stamp = now;
      const entry = cached;
      let result = cached.current;
      if (intent !== "preload") {
        result = isThenable(result)
          ? (result as Promise<unknown>).then(settle(entry, false), settle(entry, true))
          : settle(entry, false)(result);
      }
      if (inPreloadFn && isThenable(result)) (result as Promise<unknown>).catch(() => {});
      return result as ReturnType<T>;
    }
    const result = fn(...args);
    const stamp = now;
    if (cached) {
      cached.stamp = stamp;
      cached.current = result;
      cached.intent = intent;
      if (!server && intent === "navigate") cached.bump(cached.stamp);
    } else {
      const [version, bump] = signal(stamp);
      cached = {
        stamp,
        current: result,
        settled: undefined,
        hasSettled: false,
        intent,
        version,
        bump,
        refs: 0,
      };
      cache.set(key, cached);
    }
    const entry = cached;
    track();
    let outcome: unknown = result;
    if (intent !== "preload") {
      outcome = isThenable(result)
        ? (result as Promise<unknown>).then(settle(entry, false), settle(entry, true))
        : settle(entry, false)(result);
    }
    if (inPreloadFn && isThenable(outcome)) (outcome as Promise<unknown>).catch(() => {});
    return outcome as ReturnType<T>;

    function settle(entry: CacheEntry, isError: boolean): (value: unknown) => unknown {
      return (value: unknown): unknown => {
        if (isResponse(value)) return settleResponse(value, isError);
        if (isError) throw value;
        entry.settled = value;
        entry.hasSettled = true;
        return value;
      };
    }

    function settleResponse(response: Response, isError: boolean): unknown {
      const redirectTo = response.headers.get(LocationHeader);
      if (redirectTo !== null) {
        const interactive = !server && typeof window !== "undefined";
        const declared = interactive ? response.headers.get(RevalidateHeader) : null;
        const keys = declared !== null ? readRevalidateKeys(declared as string) : undefined;
        if (declared !== null) cacheKeyOp(keys, (candidate) => void (candidate.stamp = 0));
        if (navigate && redirectTo.startsWith("/")) navigate(redirectTo, { replace: true });
        else if (interactive) window.location.href = redirectTo;
        if (declared !== null) revalidate(keys, false);
        return interactive ? new Promise<never>(() => {}) : undefined;
      }
      if (isError) throw response;
      return response;
    }
  };
  cachedFn.key = name;
  cachedFn.keyFor = (...args: Parameters<T>): string => name + hashKey(args);
  return cachedFn;
}

query.get = (key: string): unknown => {
  const cached = getCache().get(key);
  if (!cached) throw new Error(`query.get: no cache entry for "${key}"`);
  return cached.hasSettled ? cached.settled : undefined;
};

query.set = <T>(key: string, value: T): void => {
  const cache = getCache();
  const now = Date.now();
  const cached = cache.get(key);
  if (cached) {
    cached.stamp = now;
    cached.current = Promise.resolve(value);
    cached.settled = value;
    cached.hasSettled = true;
    cached.intent = "preload";
  } else {
    const [version, bump] = signal(now);
    cache.set(key, {
      stamp: now,
      current: Promise.resolve(value),
      settled: value,
      hasSettled: true,
      intent: "preload",
      version,
      bump,
      refs: 0,
    });
  }
};

query.delete = (key: string): boolean => getCache().delete(key);
query.clear = (): void => {
  getCache().clear();
};
