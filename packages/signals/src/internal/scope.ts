import { FlagRecursed, FlagWatching } from "../flags";
import { disposeNode, unlink, type ReactiveNode } from "../graph";
import { root, untrack } from "../owner";
import type { ContinuationEvent, ContinuationHandle } from "./continuation";

export type { ReactiveNode };
export { parentOwner } from "../context";

export interface SourceSite {
  readonly key: string;
  readonly module: string;
  readonly ordinal: number;
  readonly line: number;
  readonly column: number;
}

export type FlushDelivery = "inline" | "scheduled";

export interface ScopeOptions {
  readonly name?: string;
  readonly controlled?: boolean;
  readonly timeoutMs?: number;
}

const DEFAULT_TIMEOUT_MS = 30_000;

export class ScopeTimeoutError extends Error {
  readonly scopeName: string | undefined;
  readonly timeoutMs: number;

  constructor(scopeName: string | undefined, timeoutMs: number) {
    super(`[reze] Scope${scopeName === undefined ? "" : ` "${scopeName}"`} did not settle within ${timeoutMs}ms`);
    this.name = "ScopeTimeoutError";
    this.scopeName = scopeName;
    this.timeoutMs = timeoutMs;
  }
}

export class ScopeDisposedError extends Error {
  readonly scopeName: string | undefined;

  constructor(scopeName: string | undefined) {
    super(`[reze] Scope${scopeName === undefined ? "" : ` "${scopeName}"`} is disposed`);
    this.name = "ScopeDisposedError";
    this.scopeName = scopeName;
  }
}

let activeScope: ExecutionScope | undefined;
let scopes: Set<ExecutionScope> | undefined;
let nodeScopes: WeakMap<ReactiveNode, ExecutionScope> | undefined;
let observer: ScopeObserver | undefined;

export function setScopeObserver(next: ScopeObserver | undefined): void {
  observer = next;
}

export function getScopeObserver(): ScopeObserver | undefined {
  return observer;
}

export function getActiveScope(): ExecutionScope | undefined {
  return activeScope;
}

export interface ScopeContext {
  readonly scope: ExecutionScope | undefined;
}

export function enterScopeContext(scope: ExecutionScope | undefined): ScopeContext {
  const prev: ScopeContext = { scope: activeScope };
  activeScope = scope;
  return prev;
}

export function restoreScopeContext(prev: ScopeContext): void {
  activeScope = prev.scope;
}
export function scopeOfNode(node: ReactiveNode): ExecutionScope | undefined {
  return nodeScopes?.get(node);
}

export function liveScopes(): readonly ExecutionScope[] {
  return scopes === undefined ? [] : [...scopes];
}

export function registerNodeScope(node: ReactiveNode, owner?: ReactiveNode | undefined): ExecutionScope | undefined {
  const scope = activeScope ?? (owner !== undefined ? nodeScopes?.get(owner) : undefined);
  if (scope === undefined) {
    return undefined;
  }
  if (scope.disposed) throw new ScopeDisposedError(scope.name);
  if (nodeScopes === undefined) {
    nodeScopes = new WeakMap();
  } else if (nodeScopes.has(node)) {
    return nodeScopes.get(node);
  }
  nodeScopes.set(node, scope);
  scope.addOwnedNode(node);
  observer?.onNodeCreated?.(node, scope, owner);
  return scope;
}

export function notifyNodeDisposed(node: ReactiveNode): void {
  nodeScopes?.get(node)?.removeOwnedNode(node);
}

export interface UniqueIdRequest {
  readonly site: SourceSite | undefined;
  readonly owner: ReactiveNode | undefined;
  readonly scope: ExecutionScope | undefined;
}

export interface ScopeObserver {
  onNodeCreated?(node: ReactiveNode, scope: ExecutionScope | undefined, owner: ReactiveNode | undefined): void;
  onContinuationEvent?(handle: ContinuationHandle, event: ContinuationEvent, site: SourceSite | undefined, value: unknown): void;
  resolveUniqueId?(request: UniqueIdRequest): string | undefined;
  renderComponent?(component: (props: never) => unknown, props: unknown): unknown;
}

interface QueueState {
  list: (ReactiveNode | undefined)[];
  length: number;
  index: number;
}
function enqueueInto(queue: QueueState, node: ReactiveNode): void {
  let insertIndex = queue.length;
  let firstInsertedIndex = insertIndex;
  let next: ReactiveNode | undefined = node;
  do {
    queue.list[insertIndex++] = next;
    next.flags &= ~FlagWatching;
    next = next.subs?.sub;
  } while (next !== undefined && next.flags & FlagWatching);
  queue.length = insertIndex;
  while (firstInsertedIndex < --insertIndex) {
    const outer = queue.list[firstInsertedIndex];
    queue.list[firstInsertedIndex++] = queue.list[insertIndex];
    queue.list[insertIndex] = outer;
  }
}

export class ExecutionScope {
  readonly name: string | undefined;
  readonly controlled: boolean;
  readonly timeoutMs: number;
  private queue: QueueState = { list: [], length: 0, index: 0 };
  private flushScheduled = false;
  private normalScheduling = false;
  private pending: Set<Promise<unknown>> | undefined;
  private waiters: Set<() => void> | undefined;
  private disposables: (() => void)[] | undefined;
  private owned: Set<ReactiveNode> | undefined;
  private _disposed = false;

  constructor(options?: ScopeOptions) {
    this.name = options?.name;
    this.controlled = options?.controlled ?? false;
    const timeoutMs = options?.timeoutMs ?? DEFAULT_TIMEOUT_MS;
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
      throw new RangeError(`[reze] Scope timeout must be finite and > 0, got ${timeoutMs}`);
    }
    this.timeoutMs = timeoutMs;
    if (scopes === undefined) {
      scopes = new Set();
    }
    scopes.add(this);
  }

  get disposed(): boolean {
    return this._disposed;
  }

  run<T>(fn: () => T): T {
    if (this._disposed) throw new ScopeDisposedError(this.name);
    const prev = activeScope;
    activeScope = this;
    try {
      return fn();
    } finally {
      activeScope = prev;
    }
  }

  usesNormalScheduling(): boolean {
    return this.normalScheduling;
  }

  hasQueuedWork(): boolean {
    return this.queue.index < this.queue.length;
  }

  enqueueNode(node: ReactiveNode): void {
    if (this._disposed) {
      return;
    }
    enqueueInto(this.queue, node);
    this.wake();
    if (!this.controlled) {
      this.scheduleFlush();
    }
  }

  pushNode(node: ReactiveNode): void {
    if (!this._disposed) {
      const queue = this.queue;
      queue.list[queue.length++] = node;
      this.wake();
    }
  }
  scheduleFlush(): void {
    if (this._disposed || (this.controlled && !this.normalScheduling) || this.flushScheduled) {
      return;
    }
    this.flushScheduled = true;
    queueMicrotask(() => {
      this.flushScheduled = false;
      if (!this._disposed) {
        this.flush("scheduled");
      }
    });
  }

  flush(delivery: FlushDelivery = "inline", deadlineTs = Number.POSITIVE_INFINITY, forceBoundary = false): boolean {
    const queue = this.queue;
    if (!forceBoundary && queue.index >= queue.length) {
      return true;
    }
    const previousScope = activeScope;
    activeScope = this;
    try {
      while (queue.index < queue.length) {
        if (Date.now() > deadlineTs) {
          return false;
        }
        const node = queue.list[queue.index]!;
        queue.list[queue.index++] = undefined;
        node.run!();
      }
    } finally {
      while (queue.index < queue.length) {
        const node = queue.list[queue.index]!;
        queue.list[queue.index++] = undefined;
        node.flags |= FlagWatching | FlagRecursed;
      }
      queue.index = 0;
      queue.length = 0;
      activeScope = previousScope;
    }
    return true;
  }

  trackPendingWork(promise: Promise<unknown>): () => void {
    if (this._disposed) throw new ScopeDisposedError(this.name);
    let released = false;
    const release = (): void => {
      if (released) return;
      released = true;
      this.pending?.delete(safe);
      this.wake();
    };
    const safe = promise.then(release, release);
    (this.pending ??= new Set()).add(safe);
    this.wake();
    return release;
  }

  pendingWork(): readonly Promise<unknown>[] {
    return this.pending === undefined ? [] : [...this.pending];
  }

  async settle(timeoutMs?: number): Promise<void> {
    const budget = timeoutMs ?? this.timeoutMs;
    if (!Number.isFinite(budget) || budget <= 0) {
      throw new RangeError(`[reze] Settle timeout must be finite and > 0, got ${budget}`);
    }
    const deadline = Date.now() + budget;
    for (;;) {
      if (this._disposed) {
        throw new ScopeDisposedError(this.name);
      }
      if (!this.flush("scheduled", deadline)) throw new ScopeTimeoutError(this.name, budget);
      if ((this.pending?.size ?? 0) === 0 && !this.hasQueuedWork()) {
        if (this._disposed) {
          throw new ScopeDisposedError(this.name);
        }
        return;
      }
      if (Date.now() >= deadline) {
        throw new ScopeTimeoutError(this.name, budget);
      }
      await this.waitForWork(deadline - Date.now());
    }
  }

  private wake(): void {
    if (this.waiters !== undefined) for (const wake of this.waiters) wake();
  }

  waitForWork(timeoutMs: number): Promise<void> {
    const { promise, resolve } = Promise.withResolvers<void>();
    const wake = (): void => {
      clearTimeout(timer);
      this.waiters?.delete(wake);
      resolve();
    };
    const timer = setTimeout(wake, Math.min(timeoutMs, 2_147_483_647));
    (this.waiters ??= new Set()).add(wake);
    return promise;
  }

  addDisposable(fn: () => void): void {
    (this.disposables ??= []).push(fn);
  }

  removeDisposable(fn: () => void): void {
    const disposables = this.disposables;
    if (disposables !== undefined) {
      const at = disposables.lastIndexOf(fn);
      if (at !== -1) {
        disposables.splice(at, 1);
      }
    }
  }

  addOwnedNode(node: ReactiveNode): void {
    if (!this._disposed) {
      (this.owned ??= new Set()).add(node);
    }
  }

  removeOwnedNode(node: ReactiveNode): void {
    this.owned?.delete(node);
  }

  activate(): void {
    this.normalScheduling = true;
    if (this.hasQueuedWork()) this.scheduleFlush();
  }

  dispose(): void {
    if (this._disposed) {
      return;
    }
    this._disposed = true;
    const queue = this.queue;
    queue.index = 0;
    queue.length = 0;
    queue.list.length = 0;
    this.flushScheduled = false;
    this.pending?.clear();
    this.wake();
    scopes?.delete(this);
    let failure: unknown;
    let hasFailure = false;
    const disposables = this.disposables;
    this.disposables = undefined;
    if (disposables !== undefined) {
      for (let i = disposables.length - 1; i >= 0; i--) {
        try {
          disposables[i]!();
        } catch (error) {
          if (!hasFailure) {
            failure = error;
            hasFailure = true;
          }
        }
      }
    }
    const owned = this.owned;
    if (owned !== undefined) {
      while (owned.size !== 0) {
        const node = owned.values().next().value!;
        owned.delete(node);
        try {
          if (node.dispose !== undefined && node.subs !== undefined) unlink(node.subs);
          else if (node.dispose !== undefined) node.dispose();
          else disposeNode(node);
        } catch (error) {
          if (!hasFailure) {
            failure = error;
            hasFailure = true;
          }
        }
      }
    }
    this.owned = undefined;
    if (hasFailure) {
      throw failure;
    }
  }
}

export function createScope(options?: ScopeOptions): ExecutionScope {
  return new ExecutionScope(options);
}

function scopedRenderRoot<T>(fn: (dispose: () => void) => T): T {
  const scope = createScope();
  const previousScope = activeScope;
  activeScope = scope;
  try {
    return root(() => fn(() => scope.dispose()));
  } catch (error) {
    try {
      scope.dispose();
    } catch (cleanupError) {
      throw new AggregateError([error, cleanupError], "render initialization and cleanup failed");
    }
    throw error;
  } finally {
    activeScope = previousScope;
  }
}

export const renderRoot = __REZE_HTML__ ? scopedRenderRoot : root;

function hostedComponent<P, R>(component: (props: P) => R, props: P): R {
  const hosted = observer?.renderComponent;
  if (hosted !== undefined) return hosted.call(observer, component as (props: never) => unknown, props) as R;
  return untrack(component, props);
}

export const runComponent: <P, R>(component: (props: P) => R, props: P) => R = __REZE_HTML__ ? hostedComponent : untrack;
