import {
  endTracking,
  reportError,
  enterEffect,
  enterOwner,
  exitEffect,
  setActiveSub,
  startTracking,
} from "./context";
import {
  FlagDirty,
  FlagHasChildEffect,
  FlagPending,
  FlagRecursedCheck,
  FlagWatching,
} from "./flags";
import { checkDirty, disposeChildren, disposeNode, type Link, type ReactiveNode } from "./graph";

/**
 * DOM binding: a single-phase effect that threads its previous return value into the next run.
 * No disposer handle and no cleanup slot; it lives exactly as long as its owner.
 */
class RenderNode<T> implements ReactiveNode {
  deps: Link | undefined = undefined;
  depsTail: Link | undefined = undefined;
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  flags: number = FlagWatching | FlagRecursedCheck;
  fn: (prev: T) => T;
  value: T;

  constructor(fn: (prev: T) => T, value: T) {
    this.fn = fn;
    this.value = value;
  }

  run(): void {
    try {
      this.runIfDirty();
    } catch (error) {
      reportError(this, error);
    }
  }

  runIfDirty(): void {
    const flags = this.flags;
    if (flags & FlagDirty || (flags & FlagPending && checkDirty(this.deps!, this))) {
      if (flags & FlagHasChildEffect) {
        disposeChildren(this);
      }
      const prevSub = startTracking(this, FlagWatching);
      try {
        enterEffect();
        this.value = this.fn(this.value);
      } finally {
        exitEffect();
        endTracking(this, prevSub);
      }
    } else if (this.deps !== undefined) {
      this.flags = FlagWatching | (flags & FlagHasChildEffect);
    }
  }

  unwatched(): void {
    disposeNode(this);
  }

  dispose(): void {
    disposeNode(this);
  }
}

/**
 * Runs `fn(init)` now and again whenever what it read changes, passing the previous result.
 * A binding that read nothing reactive and created nothing is dropped right away, so static
 * expressions cost no graph node.
 */
export function renderEffect<T>(fn: (prev: T) => T, init?: T): void {
  const node = new RenderNode(fn, init as T);
  const prevSub = enterOwner(node);
  try {
    enterEffect();
    node.value = fn(node.value);
  } finally {
    exitEffect();
    setActiveSub(prevSub);
    node.flags &= ~FlagRecursedCheck;
  }
  if (node.deps === undefined) {
    disposeNode(node);
  }
}
