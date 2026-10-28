export interface CacheOptions<A extends unknown[] = unknown[]> {
  /** Milliseconds a resolved entry stays usable, or `Infinity` until `revalidateTag` removes it. */
  readonly ttl: number;
  /** Tags `revalidateTag` matches, or a function of the call arguments that returns them. */
  readonly tags?: readonly string[] | ((...args: A) => readonly string[]);
}

interface Entry {
  readonly promise: Promise<unknown>;
  readonly expires: number;
  readonly tags: readonly string[];
}

const MaxEntries = 1024;
const stores = new WeakMap<object, Map<string, Entry>>();
const all = new Set<Map<string, Entry>>();
const isServer = typeof window === "undefined";

function keyOf(args: readonly unknown[]): string | undefined {
  try {
    return JSON.stringify(args, (_key, value: unknown) => {
      const kind = typeof value;
      if (kind === "function" || kind === "symbol" || kind === "undefined" || (kind === "number" && !Number.isFinite(value))) {
        throw new TypeError("an argument cannot be cached");
      }
      return value;
    });
  } catch {
    return undefined;
  }
}

function tagsOf<A extends unknown[]>(options: CacheOptions<A>, args: A): readonly string[] {
  if (options.tags === undefined) return [];
  return typeof options.tags === "function" ? options.tags(...args) : options.tags;
}

function sweep(store: Map<string, Entry>): void {
  if (store.size <= MaxEntries) return;
  const now = Date.now();
  for (const [key, entry] of store) {
    if (entry.expires <= now) store.delete(key);
  }
  while (store.size > MaxEntries) {
    const first = store.keys().next();
    if (first.done === true) return;
    store.delete(first.value);
  }
}

/**
 * Caches an async loader on the server by its call arguments. A hit returns the same promise, so
 * concurrent renders share one load. The browser calls the loader directly. Arguments must be
 * JSON-serializable to be cached; anything else calls through.
 */
export function cache<A extends unknown[], R>(
  load: (...args: A) => R | PromiseLike<R>,
  options: CacheOptions<A>,
): (...args: A) => Promise<Awaited<R>> {
  if (options.ttl !== Infinity && (!Number.isFinite(options.ttl) || options.ttl < 0)) {
    throw new Error(`[reze] cache ttl must be a non-negative number of milliseconds or Infinity, got ${String(options.ttl)}`);
  }
  if (!isServer) return (...args: A) => Promise.resolve(load(...args));
  let store = stores.get(load);
  if (store === undefined) {
    store = new Map();
    stores.set(load, store);
    all.add(store);
  }
  const owned = store;
  return (...args: A): Promise<Awaited<R>> => {
    const key = keyOf(args);
    if (key === undefined) return Promise.resolve(load(...args));
    const hit = owned.get(key);
    if (hit !== undefined) {
      if (hit.expires > Date.now()) return hit.promise as Promise<Awaited<R>>;
      owned.delete(key);
    }
    const promise = Promise.resolve(load(...args)) as Promise<Awaited<R>>;
    owned.set(key, {
      promise,
      expires: options.ttl === Infinity ? Infinity : Date.now() + options.ttl,
      tags: tagsOf(options, args),
    });
    sweep(owned);
    promise.catch(() => {
      if (owned.get(key)?.promise === promise) owned.delete(key);
    });
    return promise;
  };
}

/** Drops every cached entry tagged with `tag`; returns how many entries were removed. */
export function revalidateTag(tag: string): number {
  let removed = 0;
  for (const store of all) {
    for (const [key, entry] of store) {
      if (!entry.tags.includes(tag)) continue;
      store.delete(key);
      removed++;
    }
  }
  return removed;
}
