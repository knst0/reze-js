import { adopt, getOwner, setActiveOwner, setActiveSub } from "./context";
import { FlagNone } from "./flags";
import { disposeNode, type Link, type ReactiveNode } from "./graph";
import { endBatch, startBatch } from "./scheduler";

/** Opaque handle to a node that owns computations (`root`, `effect`, `computed`, render bindings). */
export type Owner = ReactiveNode;

class RootNode implements ReactiveNode {
  deps: Link | undefined = undefined;
  depsTail: Link | undefined = undefined;
  flags: number = FlagNone;
}

class CleanupNode implements ReactiveNode {
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  flags: number = FlagNone;
  fn: () => void;

  constructor(fn: () => void) {
    this.fn = fn;
  }

  /** Present so the owner releases this node before it re-runs, like an owned effect. */
  dispose(): void {}

  /** The owner unlinked this node: it is re-running or being disposed. */
  unwatched(): void {
    const prevSub = setActiveSub(undefined);
    try {
      this.fn();
    } finally {
      setActiveSub(prevSub);
    }
  }
}

/**
 * Runs `fn` in a new owner detached from the current one. Everything created inside lives until
 * `dispose` is called. Not a batch: list rows are roots created mid-flush, and ending a batch
 * there would flush re-entrantly.
 */
export function root<T>(fn: (dispose: () => void) => T): T {
  const node = new RootNode();
  const prevSub = setActiveSub(undefined);
  const prevOwner = setActiveOwner(node);
  try {
    return fn(() => disposeNode(node));
  } finally {
    setActiveSub(prevSub);
    setActiveOwner(prevOwner);
  }
}

/** Registers `fn` to run when the current owner re-runs or is disposed. No-op without an owner. */
export function onCleanup(fn: () => void): void {
  const owner = getOwner();
  if (owner !== undefined) {
    adopt(new CleanupNode(fn), owner);
  }
}

/** Runs `fn` without tracking reads; computations created inside stay owned by the current owner. */
export function untrack<T>(fn: () => T): T {
  const prevSub = setActiveSub(undefined);
  if (prevSub === undefined) {
    return fn();
  }
  const prevOwner = setActiveOwner(prevSub);
  try {
    return fn();
  } finally {
    setActiveSub(prevSub);
    setActiveOwner(prevOwner);
  }
}

/** Defers effects until the outermost batch exits. */
export function batch<T>(fn: () => T): T {
  startBatch();
  try {
    return fn();
  } finally {
    endBatch();
  }
}

/** Runs `fn` untracked under `owner`; for extensions that resume work after an `await`. */
export function runWithOwner<T>(owner: Owner | undefined, fn: () => T): T {
  const prevSub = setActiveSub(undefined);
  const prevOwner = setActiveOwner(owner);
  try {
    return fn();
  } finally {
    setActiveSub(prevSub);
    setActiveOwner(prevOwner);
  }
}
