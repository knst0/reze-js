// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
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

class EffectNode implements ReactiveNode {
  deps: Link | undefined = undefined;
  depsTail: Link | undefined = undefined;
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  flags: number = FlagWatching | FlagRecursedCheck;
  cleanup: (() => void) | void = undefined;
  fn: () => (() => void) | void;

  constructor(fn: () => (() => void) | void) {
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
      if (flags & FlagHasChildEffect) {
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
      this.flags = FlagWatching | (flags & FlagHasChildEffect);
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

export function effect(fn: () => void | (() => void)): () => void {
  const e = new EffectNode(fn);
  const prevSub = enterOwner(e);
  try {
    enterEffect();
    e.cleanup = e.fn();
  } finally {
    exitEffect();
    setActiveSub(prevSub);
    e.flags &= ~FlagRecursedCheck;
  }
  return effectOper.bind(e);
}

export function isEffect(fn: () => void): boolean {
  return fn.name === "bound " + effectOper.name;
}

function effectOper(this: EffectNode): void {
  this.dispose();
}

function runCleanup(e: EffectNode): void {
  const cleanup = e.cleanup!;
  e.cleanup = undefined;
  const prevSub = setActiveSub(undefined);
  try {
    cleanup();
  } finally {
    setActiveSub(prevSub);
  }
}
