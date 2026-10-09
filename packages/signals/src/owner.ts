import { adopt, getOwner, setActiveOwner, setActiveSub } from "./context";
import { FlagNone } from "./flags";
import { disposeNode, type Link, type ReactiveNode } from "./graph";
import { notifyNodeDisposed, registerNodeScope, type ExecutionScope } from "./internal/scope";
import { profileCreated } from "./profile";

/** Opaque handle to a node that owns computations (`root`, `effect`, `computed`, render bindings). */
export type Owner = ReactiveNode;

class RootNode implements ReactiveNode {
  declare deps: Link | undefined;
  declare depsTail: Link | undefined;
  declare flags: number;
  declare parent: ReactiveNode | undefined;

  constructor(parent: ReactiveNode | undefined) {
    this.deps = undefined;
    this.depsTail = undefined;
    this.flags = FlagNone;
    this.parent = parent;
  }
}

class CleanupNode implements ReactiveNode {
  declare subs: Link | undefined;
  declare subsTail: Link | undefined;
  declare flags: number;
  declare fn: () => void;

  constructor(fn: () => void) {
    this.subs = undefined;
    this.subsTail = undefined;
    this.flags = FlagNone;
    this.fn = fn;
  }

  dispose(): void {}

  unwatched(): void {
    const prevSub = setActiveSub(undefined);
    try {
      this.fn();
    } finally {
      setActiveSub(prevSub);
      if (__REZE_HTML__) notifyNodeDisposed(this);
    }
  }
}

/**
 * Runs `fn` untracked in a new owner detached from the current one. Everything created inside
 * lives until `dispose` is called; it still sees the current owner's context.
 */
export function root<T>(fn: (dispose: () => void) => T): T {
  const node = new RootNode(getOwner());
  if (process.env.NODE_ENV !== "production") {
    profileCreated(node, "root", undefined);
  }
  let scope: ExecutionScope | undefined;
  let scopeDispose: (() => void) | undefined;
  if (__REZE_HTML__) {
    scope = registerNodeScope(node, node.parent);
    if (scope !== undefined) {
      scopeDispose = (): void => {
        disposeNode(node);
      };
      scope.addDisposable(scopeDispose);
    }
  }
  const prevSub = setActiveSub(undefined);
  const prevOwner = setActiveOwner(node);
  const dispose = (): void => {
    if (scope !== undefined && scopeDispose !== undefined) {
      scope.removeDisposable(scopeDispose);
    }
    disposeNode(node);
  };
  try {
    return fn(dispose);
  } finally {
    setActiveSub(prevSub);
    setActiveOwner(prevOwner);
  }
}

/**
 * Registers `fn` to run, untracked, when the current owner re-runs or is disposed; cleanups and
 * owned nodes are released newest first. No-op without an owner.
 */
export function onCleanup(fn: () => void): void {
  const owner = getOwner();
  if (owner !== undefined) {
    adopt(new CleanupNode(fn), owner);
  }
}

/**
 * Runs `fn` without tracking reads; computations created inside stay owned by the current owner.
 * `untrack(fn, arg)` calls `fn(arg)`, which saves the closure `untrack(() => fn(arg))` allocates.
 */
export function untrack<T>(fn: () => T): T;
export function untrack<T, A>(fn: (arg: A) => T, arg: A): T;
export function untrack<T, A>(fn: (arg?: A) => T, arg?: A): T {
  const prevSub = setActiveSub(undefined);
  if (prevSub === undefined) {
    return fn(arg);
  }
  const prevOwner = setActiveOwner(prevSub);
  try {
    return fn(arg);
  } finally {
    setActiveSub(prevSub);
    setActiveOwner(prevOwner);
  }
}
