/** Finite primitive segments identify query results and support prefix invalidation. */
export type QueryKey = readonly (string | number | boolean | null)[];
export interface QueryFunctionContext {
  readonly key: QueryKey;
  /**
   * Aborted when the query is replaced or the client is cleared. A replaced query is aborted
   * before it starts when the replacement happens first, so check `signal.aborted` as well as
   * listening for the event.
   */
  readonly signal: AbortSignal;
}

export interface QueryOptions<TData> {
  readonly key: QueryKey;
  readonly queryFn: (context: QueryFunctionContext) => TData | PromiseLike<TData>;
  /** Freshness window in milliseconds; defaults to the client's stale time. `Infinity` never expires. */
  readonly staleTime?: number;
  /** Inactive retention window in milliseconds; defaults to the client's GC time. `Infinity` never collects. */
  readonly gcTime?: number;
}

export interface QueryClientOptions {
  /** Freshness window in milliseconds; defaults to zero. `Infinity` never expires. */
  readonly staleTime?: number;
  /** Inactive retention window in milliseconds; defaults to five minutes. `Infinity` never collects. */
  readonly gcTime?: number;
}

export interface DehydratedQuery {
  readonly key: QueryKey;
  readonly data: unknown;
  /** Milliseconds since the last update when dehydrated, so restoring never compares clocks. */
  readonly ageMs: number;
  readonly invalidated: boolean;
}

export type DehydratedState = readonly DehydratedQuery[];

interface QueryEntry {
  key: QueryKey;
  data: unknown;
  hasData: boolean;
  updatedAt: number;
  lastUsedAt: number;
  staleTime: number;
  gcTime: number;
  invalidated: boolean;
  invalidationVersion: number;
  requestVersion: number;
  pendingInvalidationVersion: number;
  pending: Promise<unknown> | undefined;
  controller: AbortController | undefined;
}

const DefaultGcTime = 5 * 60_000;

function assertDuration(name: string, value: number): void {
  if (!(value >= 0)) {
    throw new RangeError(`Query ${name} must be a non-negative number`);
  }
}

function keyId(key: QueryKey): string {
  if (!Array.isArray(key)) throw new TypeError("Query key must be an array");
  for (const part of key) {
    const type = typeof part;
    if (part !== null && type !== "string" && type !== "boolean" && type !== "number") {
      throw new TypeError("Query keys can only contain strings, numbers, booleans, and null");
    }
    if (type === "number" && !Number.isFinite(part)) {
      throw new TypeError("Query keys cannot contain non-finite numbers");
    }
  }
  return JSON.stringify(key.map((part) => [typeof part, typeof part === "number" && Object.is(part, -0) ? "-0" : part]));
}

function createEntry(key: QueryKey, now: number, staleTime: number, gcTime: number): QueryEntry {
  return {
    key: [...key],
    data: undefined,
    hasData: false,
    updatedAt: 0,
    lastUsedAt: now,
    staleTime,
    gcTime,
    invalidated: false,
    invalidationVersion: 0,
    requestVersion: 0,
    pendingInvalidationVersion: 0,
    pending: undefined,
    controller: undefined,
  };
}

/** Owns an isolated in-memory query cache; create one per browser app or server request. */
export class QueryClient {
  readonly #entries = new Map<string, QueryEntry>();
  readonly #staleTime: number;
  readonly #gcTime: number;
  #nextCollectionAt = Number.POSITIVE_INFINITY;

  constructor(options: QueryClientOptions = {}) {
    this.#staleTime = options.staleTime ?? 0;
    this.#gcTime = options.gcTime ?? DefaultGcTime;
    assertDuration("staleTime", this.#staleTime);
    assertDuration("gcTime", this.#gcTime);
  }

  /**
   * Reuses fresh data or in-flight work; failed results are not cached. Work started before an
   * invalidation is not reused. Invalid options reject instead of throwing. When a query is
   * replaced by `setQueryData`, `hydrate`, or a newer request while it is pending and its
   * `queryFn` fails (for example by honoring the abort signal), callers receive the replacement's
   * result instead of the failure.
   */
  fetchQuery<TData>(options: QueryOptions<TData>): Promise<TData> {
    try {
      return this.#fetch(options);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  /** Returns cached data even when stale; use `fetchQuery` to enforce freshness. */
  getQueryData<TData>(key: QueryKey): TData | undefined {
    const now = Date.now();
    this.#collect(now);
    const entry = this.#entries.get(keyId(key));
    if (entry === undefined || !entry.hasData) return undefined;
    entry.lastUsedAt = now;
    this.#nextCollectionAt = Math.min(this.#nextCollectionAt, now + entry.gcTime);
    return entry.data as TData;
  }

  /** Stores fresh data and prevents an older in-flight response from replacing it. */
  setQueryData<TData>(key: QueryKey, data: TData): TData {
    const id = keyId(key);
    const now = Date.now();
    this.#collect(now);
    let entry = this.#entries.get(id);
    if (entry === undefined) {
      entry = createEntry(key, now, this.#staleTime, this.#gcTime);
      this.#entries.set(id, entry);
    }
    entry.requestVersion++;
    entry.controller?.abort();
    entry.controller = undefined;
    entry.pending = undefined;
    entry.data = data;
    entry.hasData = true;
    entry.updatedAt = now;
    entry.lastUsedAt = now;
    entry.invalidated = false;
    this.#nextCollectionAt = Math.min(this.#nextCollectionAt, now + entry.gcTime);
    return data;
  }

  /** Marks all queries, or queries beginning with `prefix`, stale. */
  invalidateQueries(prefix?: QueryKey): void {
    if (prefix !== undefined) keyId(prefix);
    for (const entry of this.#entries.values()) {
      let matches = prefix === undefined || prefix.length <= entry.key.length;
      for (let i = 0; matches && prefix !== undefined && i < prefix.length; i++) {
        matches = Object.is(entry.key[i], prefix[i]);
      }
      if (!matches) continue;
      entry.invalidated = true;
      entry.invalidationVersion++;
    }
  }

  /** Returns settled data for transport; callers must use a safe serializer for the values. */
  dehydrate(): DehydratedState {
    const now = Date.now();
    this.#collect(now);
    const state: DehydratedQuery[] = [];
    for (const entry of this.#entries.values()) {
      if (!entry.hasData) continue;
      state.push({
        key: [...entry.key],
        data: entry.data,
        ageMs: Math.max(0, now - entry.updatedAt),
        invalidated: entry.invalidated,
      });
    }
    return state;
  }

  /**
   * Restores snapshots newer than this client's data. The whole state is validated first, so an
   * invalid entry throws a `TypeError` and restores nothing.
   */
  hydrate(state: DehydratedState): void {
    if (!Array.isArray(state)) throw new TypeError("Dehydrated state must be an array");
    const ids = state.map((query) => {
      if (typeof query !== "object" || query === null) throw new TypeError("Dehydrated query must be an object");
      if (typeof query.ageMs !== "number" || !Number.isFinite(query.ageMs) || query.ageMs < 0) {
        throw new TypeError("Dehydrated query ageMs must be a finite non-negative number");
      }
      if (typeof query.invalidated !== "boolean") throw new TypeError("Dehydrated query invalidated must be a boolean");
      return keyId(query.key);
    });

    const now = Date.now();
    this.#collect(now);
    for (let i = 0; i < state.length; i++) {
      const query = state[i]!;
      const id = ids[i]!;
      const updatedAt = now - query.ageMs;
      const current = this.#entries.get(id);
      if (current?.hasData && current.updatedAt >= updatedAt) continue;
      current?.controller?.abort();
      const entry = createEntry(query.key, now, this.#staleTime, this.#gcTime);
      entry.data = query.data;
      entry.hasData = true;
      entry.updatedAt = updatedAt;
      entry.invalidated = query.invalidated;
      entry.requestVersion = (current?.requestVersion ?? 0) + 1;
      this.#entries.set(id, entry);
      this.#nextCollectionAt = Math.min(this.#nextCollectionAt, now + this.#gcTime);
    }
  }

  /** Aborts pending query functions and removes every cached result. */
  clear(): void {
    for (const entry of this.#entries.values()) entry.controller?.abort();
    this.#entries.clear();
    this.#nextCollectionAt = Number.POSITIVE_INFINITY;
  }

  #fetch<TData>(options: QueryOptions<TData>): Promise<TData> {
    const id = keyId(options.key);
    const staleTime = options.staleTime ?? this.#staleTime;
    const gcTime = options.gcTime ?? this.#gcTime;
    assertDuration("staleTime", staleTime);
    assertDuration("gcTime", gcTime);

    const now = Date.now();
    this.#collect(now);
    let entry = this.#entries.get(id);
    if (entry === undefined) {
      entry = createEntry(options.key, now, staleTime, gcTime);
      this.#entries.set(id, entry);
    }
    entry.lastUsedAt = now;
    entry.staleTime = staleTime;
    entry.gcTime = gcTime;
    this.#nextCollectionAt = Math.min(this.#nextCollectionAt, now + gcTime);
    if (entry.pending !== undefined && entry.pendingInvalidationVersion === entry.invalidationVersion) {
      return entry.pending as Promise<TData>;
    }
    if (entry.hasData && !entry.invalidated && now - entry.updatedAt < staleTime) {
      return Promise.resolve(entry.data as TData);
    }

    entry.controller?.abort();
    const controller = new AbortController();
    const requestVersion = ++entry.requestVersion;
    const invalidationVersion = entry.invalidationVersion;
    entry.controller = controller;
    entry.pendingInvalidationVersion = invalidationVersion;
    const target = entry;
    const pending = Promise.resolve()
      .then(() => {
        if (controller.signal.aborted) throw controller.signal.reason;
        return options.queryFn({ key: target.key, signal: controller.signal });
      })
      .then(
        (data) => {
          if (this.#entries.get(id) === target && target.requestVersion === requestVersion) {
            target.data = data;
            target.hasData = true;
            target.updatedAt = Date.now();
            target.invalidated = target.invalidationVersion !== invalidationVersion;
            target.lastUsedAt = target.updatedAt;
          }
          return data;
        },
        (error: unknown) => this.#followReplacement<TData>(id, target, requestVersion, error),
      )
      .finally(() => {
        if (this.#entries.get(id) === target && target.requestVersion === requestVersion) {
          target.pending = undefined;
          target.controller = undefined;
          this.#nextCollectionAt = Math.min(this.#nextCollectionAt, target.lastUsedAt + target.gcTime);
        }
      });
    entry.pending = pending;
    return pending;
  }

  #followReplacement<TData>(id: string, entry: QueryEntry, requestVersion: number, error: unknown): TData | Promise<TData> {
    const current = this.#entries.get(id);
    if (current === undefined || (current === entry && entry.requestVersion === requestVersion)) throw error;
    if (current.pending !== undefined) return current.pending as Promise<TData>;
    if (current.hasData) return current.data as TData;
    throw error;
  }

  #collect(now: number): void {
    if (now < this.#nextCollectionAt) return;
    let nextCollectionAt = Number.POSITIVE_INFINITY;
    for (const [id, entry] of this.#entries) {
      if (entry.pending !== undefined) continue;
      const expiresAt = entry.lastUsedAt + entry.gcTime;
      if (expiresAt <= now) this.#entries.delete(id);
      else nextCollectionAt = Math.min(nextCollectionAt, expiresAt);
    }
    this.#nextCollectionAt = nextCollectionAt;
  }
}

/** Creates an independent cache scope. */
export function createQueryClient(options?: QueryClientOptions): QueryClient {
  return new QueryClient(options);
}
