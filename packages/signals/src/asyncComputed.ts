import { adopt, enterEffect, exitEffect, getOwner, reportError, setActiveSub, startTracking, trackPendingRead } from "./context";
import { debugHook } from "./devtools";
import { FlagDirty, FlagNone, FlagOwnsChildren, FlagPending, FlagRecursedCheck, FlagWatching } from "./flags";
import { checkDirty, disposeChildren, disposeNode, type Link, purgeDeps, type ReactiveNode } from "./graph";
import { SignalNode } from "./signal";

/** The tracking context the compiler threads through an async computation. */
export interface AsyncContext {
  /**
   * Reads `source` as a dependency of this run, also after an `await`. Once a newer run started or
   * the computation was disposed, reads are untracked.
   */
  get<T>(source: () => T): T;
}

/** The state of an async computation; every getter is tracked. */
export interface AsyncComputed<T> {
  /**
   * The latest resolved value; `undefined` until the first run resolves. Kept while a re-run is pending.
   * A tracked read before the first run settles makes the nearest enclosing `boundary` pending until the
   * reader re-runs or is disposed.
   */
  value(): T | undefined;
  /** Whether the latest run has not settled yet. */
  isPending(): boolean;
  /** The rejection of the latest run; cleared when a later run resolves. */
  error(): unknown;
}

class AsyncRun implements AsyncContext {
  node: AsyncComputedNode<unknown>;
  generation: number;

  constructor(node: AsyncComputedNode<unknown>, generation: number) {
    this.node = node;
    this.generation = generation;
  }

  get<T>(source: () => T): T {
    const node = this.node;
    const prevSub = setActiveSub(node.generation === this.generation && node.flags !== FlagNone ? node : undefined);
    try {
      return source();
    } finally {
      setActiveSub(prevSub);
    }
  }
}

class AsyncComputedNode<T> implements ReactiveNode, AsyncComputed<T> {
  deps: Link | undefined;
  depsTail: Link | undefined;
  subs: Link | undefined;
  subsTail: Link | undefined;
  flags: number;
  generation: number;
  fn: (c: AsyncContext) => PromiseLike<T> | T;
  resolved: SignalNode<T | undefined>;
  pending: SignalNode<boolean>;
  hasSettled: boolean;
  rejection: SignalNode<unknown>;

  constructor(fn: (c: AsyncContext) => PromiseLike<T> | T) {
    this.deps = undefined;
    this.depsTail = undefined;
    this.subs = undefined;
    this.subsTail = undefined;
    this.flags = FlagWatching;
    this.generation = 0;
    this.fn = fn;
    this.resolved = new SignalNode<T | undefined>(undefined, Object.is);
    this.pending = new SignalNode<boolean>(true, Object.is);
    this.hasSettled = false;
    this.rejection = new SignalNode<unknown>(undefined, Object.is);
  }

  value(): T | undefined {
    if (!this.hasSettled) {
      this.pending.read();
      trackPendingRead();
    }
    return this.resolved.read();
  }

  isPending(): boolean {
    return this.pending.read();
  }

  error(): unknown {
    return this.rejection.read();
  }

  run(): void {
    try {
      const flags = this.flags;
      if (flags & FlagDirty || (flags & FlagPending && checkDirty(this.deps!, this))) {
        this.start();
      } else if (this.deps !== undefined) {
        this.flags = FlagWatching | (flags & FlagOwnsChildren);
      }
    } catch (error) {
      reportError(this, error);
    }
  }

  start(): void {
    if (this.flags & FlagOwnsChildren) {
      disposeChildren(this);
    }
    const generation = ++this.generation;
    this.pending.write(true);
    const prevSub = startTracking(this, FlagWatching);
    let result: PromiseLike<T> | T;
    try {
      enterEffect();
      result = this.fn(new AsyncRun(this as AsyncComputedNode<unknown>, generation));
    } catch (error) {
      result = Promise.reject(error);
    } finally {
      exitEffect();
      setActiveSub(prevSub);
      this.flags &= ~FlagRecursedCheck;
    }
    Promise.resolve(result).then(
      (value) => {
        if (generation === this.generation) {
          this.hasSettled = true;
          purgeDeps(this);
          this.resolved.write(value);
          this.rejection.write(undefined);
          this.pending.write(false);
        }
      },
      (error: unknown) => {
        if (generation === this.generation) {
          this.hasSettled = true;
          purgeDeps(this);
          this.rejection.write(error);
          this.pending.write(false);
        }
      },
    );
  }

  unwatched(): void {
    this.dispose();
  }

  dispose(): void {
    ++this.generation;
    disposeNode(this);
  }
}

/**
 * Runs `fn` now, and again on the next flush whenever a source it read synchronously or through
 * `c.get` changes. A settlement of a superseded run, or one after the owner disposed the
 * computation, is dropped. A rejection or a synchronous throw is stored in `error()`, not rethrown.
 */
export function asyncComputed<T>(fn: (c: AsyncContext) => PromiseLike<T> | T): AsyncComputed<T> {
  const node = new AsyncComputedNode(fn);
  if (process.env.NODE_ENV !== "production" && debugHook !== undefined) {
    debugHook.created(node, "async", undefined, () => node.resolved.pendingValue);
  }
  const owner = getOwner();
  if (owner !== undefined) {
    adopt(node, owner);
  }
  node.start();
  return node;
}
