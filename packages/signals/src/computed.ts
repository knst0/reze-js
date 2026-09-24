// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { endTracking, getOwner, setActiveSub, startTracking, track } from "./context";
import {
  FlagDirty,
  FlagHasChildEffect,
  FlagMutable,
  FlagNone,
  FlagPending,
  FlagRecursedCheck,
} from "./flags";
import {
  checkDirty,
  disposeAllDepsInReverse,
  disposeChildren,
  type Link,
  type ReactiveNode,
  shallowPropagate,
} from "./graph";

class ComputedNode<T = unknown> implements ReactiveNode {
  value: T | undefined = undefined;
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  deps: Link | undefined = undefined;
  depsTail: Link | undefined = undefined;
  flags: number = FlagNone;
  getter: (previousValue?: T) => T;
  parent: ReactiveNode | undefined;

  constructor(getter: (previousValue?: T) => T, parent: ReactiveNode | undefined) {
    this.getter = getter;
    this.parent = parent;
  }

  update(): boolean {
    if (this.flags & FlagHasChildEffect) {
      disposeChildren(this);
    }
    const prevSub = startTracking(this, FlagMutable);
    try {
      const oldValue = this.value;
      return oldValue !== (this.value = this.getter(oldValue));
    } finally {
      endTracking(this, prevSub);
    }
  }

  unwatched(): void {
    if (this.depsTail !== undefined) {
      this.flags = FlagMutable | FlagDirty;
      disposeAllDepsInReverse(this);
    }
  }
}

export function computed<T>(getter: (previousValue?: T) => T): () => T {
  return (computedOper<T>).bind(new ComputedNode(getter, getOwner()));
}

export function isComputed(fn: () => void): boolean {
  return fn.name === "bound " + computedOper.name;
}

function computedOper<T>(this: ComputedNode<T>): T {
  const flags = this.flags;
  if (process.env.NODE_ENV !== "production" && flags & FlagRecursedCheck) {
    console.warn(
      "[rezejs] Cycle detected: a computed was read while it is being evaluated, so it depends " +
        "on itself and returns a stale value. Computed graphs must be acyclic.",
    );
  }
  if (
    flags & FlagDirty ||
    (flags & FlagPending &&
      (checkDirty(this.deps!, this) || ((this.flags = flags & ~FlagPending), false)))
  ) {
    if (this.update()) {
      const subs = this.subs;
      if (subs !== undefined) {
        shallowPropagate(subs);
      }
    }
  } else if (!flags) {
    this.flags = FlagMutable | FlagRecursedCheck;
    const prevSub = setActiveSub(this);
    try {
      this.value = this.getter();
    } finally {
      setActiveSub(prevSub);
      this.flags &= ~FlagRecursedCheck;
    }
  }
  track(this);
  return this.value!;
}
