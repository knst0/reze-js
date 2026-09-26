// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { endTracking, enterEffect, enterOwner, exitEffect, reportError, setActiveSub, startTracking } from "./context";
import { debugHook } from "./devtools";
import { FlagDirty, FlagOwnsChildren, FlagPending, FlagRecursedCheck, FlagWatching } from "./flags";
import { checkDirty, disposeChildren, disposeNode, type Link, type ReactiveNode } from "./graph";

type EffectCleanup = (() => void) | void;

class EffectNode implements ReactiveNode {
  deps: Link | undefined = undefined;
  depsTail: Link | undefined = undefined;
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  flags: number = FlagWatching | FlagRecursedCheck;
  cleanup: EffectCleanup = undefined;
  fn: () => EffectCleanup;

  constructor(fn: () => EffectCleanup) {
    this.fn = fn;
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
      if (flags & FlagOwnsChildren) {
        disposeChildren(this);
      }
      if (this.cleanup) {
        runCleanup(this);
        if (!this.flags) {
          return;
        }
      }
      const prevSub = startTracking(this, FlagWatching);
      try {
        enterEffect();
        this.cleanup = this.fn();
      } finally {
        exitEffect();
        endTracking(this, prevSub);
      }
    } else if (this.deps !== undefined) {
      this.flags = FlagWatching | (flags & FlagOwnsChildren);
    }
  }

  unwatched(): void {
    this.dispose();
  }

  dispose(): void {
    disposeNode(this);
    if (this.cleanup) {
      runCleanup(this);
    }
  }
}

/**
 * Runs `fn` now and again, on the next flush, whenever what it read changes. The function `fn`
 * returns runs before the next run and on disposal. Nested nodes are disposed first, newest first.
 * Returns the disposer.
 */
export function effect(fn: () => EffectCleanup): () => void {
  const node = new EffectNode(fn);
  if (process.env.NODE_ENV !== "production" && debugHook !== undefined) {
    debugHook.created(node, "effect", undefined, () => undefined);
  }
  const prevSub = enterOwner(node);
  try {
    enterEffect();
    node.cleanup = fn();
  } finally {
    exitEffect();
    setActiveSub(prevSub);
    node.flags &= ~FlagRecursedCheck;
  }
  return disposeEffect.bind(node);
}

function disposeEffect(this: EffectNode): void {
  this.dispose();
}

function runCleanup(node: EffectNode): void {
  const cleanup = node.cleanup!;
  node.cleanup = undefined;
  const prevSub = setActiveSub(undefined);
  try {
    cleanup();
  } finally {
    setActiveSub(prevSub);
  }
}
