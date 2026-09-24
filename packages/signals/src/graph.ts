/*
 * Reactive graph core, ported from alien-signals
 * (https://github.com/stackblitz/alien-signals).
 *
 * MIT License
 *
 * Copyright (c) 2024-present Johnson Chu
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

/*
 * Differences from upstream: node kinds dispatch through methods on the node
 * (`update`, `run`, `unwatched`, `dispose`) instead of callbacks captured by
 * `createReactiveSystem`, and the runtime state lives in context.ts and
 * scheduler.ts. This module references no concrete node kind, so a bundle
 * only contains the kinds it actually constructs.
 */

import {
  FlagDirty,
  FlagMutable,
  FlagNone,
  FlagPending,
  FlagRecursed,
  FlagRecursedCheck,
  FlagWatching,
} from "./flags";
import { scheduleNode } from "./scheduler";

export interface ReactiveNode {
  deps?: Link;
  depsTail?: Link;
  subs?: Link;
  subsTail?: Link;
  flags: number;
  /** Required on `FlagMutable` nodes: settle pending state, return whether the value changed. */
  update?(): boolean;
  /** Required on `FlagWatching` nodes: executed by `flush` after being queued. */
  run?(): void;
  /** Called when the last subscriber unlinks. */
  unwatched?(): void;
  /** Marks owned nodes (effects, scopes): disposed before their owner re-runs. */
  dispose?(): void;
}

export interface Link {
  version: number;
  dep: ReactiveNode;
  sub: ReactiveNode;
  prevSub: Link | undefined;
  nextSub: Link | undefined;
  prevDep: Link | undefined;
  nextDep: Link | undefined;
}

/**
 * Pending traversal forks. `propagate`/`checkDirty` keep these in function-local
 * arrays (P01): one allocation per call instead of one linked record per fork,
 * and locals stay correct when user getters re-enter the graph mid-traversal.
 */
type ForkStack = Array<Link | undefined>;

/** Unlinks owned nodes from `sub` (triggering their disposal), newest first. */
export function disposeChildren(sub: ReactiveNode): void {
  let link = sub.depsTail;
  while (link !== undefined) {
    const prev = link.prevDep;
    if (link.dep.dispose !== undefined) {
      unlink(link, sub);
    }
    link = prev;
  }
}

/** Detaches `node` from its deps and its owner. */
export function disposeNode(node: ReactiveNode): void {
  node.flags = FlagNone;
  disposeAllDepsInReverse(node);
  const sub = node.subs;
  if (sub !== undefined) {
    unlink(sub);
  }
}

export function disposeAllDepsInReverse(sub: ReactiveNode): void {
  let link = sub.depsTail;
  while (link !== undefined) {
    const prev = link.prevDep;
    unlink(link, sub);
    link = prev;
  }
}

export function purgeDeps(sub: ReactiveNode): void {
  const depsTail = sub.depsTail;
  let dep = depsTail !== undefined ? depsTail.nextDep : sub.deps;
  while (dep !== undefined) {
    dep = unlink(dep, sub);
  }
}

export function link(dep: ReactiveNode, sub: ReactiveNode, version: number): void {
  const prevDep = sub.depsTail;
  if (prevDep !== undefined && prevDep.dep === dep) {
    return;
  }
  const nextDep = prevDep !== undefined ? prevDep.nextDep : sub.deps;
  if (nextDep !== undefined && nextDep.dep === dep) {
    nextDep.version = version;
    sub.depsTail = nextDep;
    return;
  }
  const prevSub = dep.subsTail;
  if (prevSub !== undefined && prevSub.version === version && prevSub.sub === sub) {
    return;
  }
  const newLink: Link =
    (sub.depsTail =
    dep.subsTail =
      {
        version,
        dep,
        sub,
        prevDep,
        nextDep,
        prevSub,
        nextSub: undefined,
      });
  if (nextDep !== undefined) {
    nextDep.prevDep = newLink;
  }
  if (prevDep !== undefined) {
    prevDep.nextDep = newLink;
  } else {
    sub.deps = newLink;
  }
  if (prevSub !== undefined) {
    prevSub.nextSub = newLink;
  } else {
    dep.subs = newLink;
  }
}

export function unlink(link: Link, sub = link.sub): Link | undefined {
  const { dep, prevDep, nextDep, nextSub, prevSub } = link;
  if (nextDep !== undefined) {
    nextDep.prevDep = prevDep;
  } else {
    sub.depsTail = prevDep;
  }
  if (prevDep !== undefined) {
    prevDep.nextDep = nextDep;
  } else {
    sub.deps = nextDep;
  }
  if (nextSub !== undefined) {
    nextSub.prevSub = prevSub;
  } else {
    dep.subsTail = prevSub;
  }
  if (prevSub !== undefined) {
    prevSub.nextSub = nextSub;
  } else if ((dep.subs = nextSub) === undefined) {
    dep.unwatched?.();
  }
  return nextDep;
}
export function propagate(link: Link, innerWrite: boolean): void {
  let next = link.nextSub;
  const stack: ForkStack = [];

  top: for (;;) {
    const sub = link.sub;
    let flags = sub.flags;

    if (!(flags & (FlagRecursedCheck | FlagRecursed | FlagDirty | FlagPending))) {
      sub.flags = flags | FlagPending;
      if (innerWrite) {
        sub.flags |= FlagRecursed;
      }
    } else if (!(flags & (FlagRecursedCheck | FlagRecursed))) {
      flags = FlagNone;
    } else if (!(flags & FlagRecursedCheck)) {
      sub.flags = (flags & ~FlagRecursed) | FlagPending;
    } else if (!(flags & (FlagDirty | FlagPending)) && isValidLink(link, sub)) {
      sub.flags = flags | (FlagRecursed | FlagPending);
      flags &= FlagMutable;
    } else {
      flags = FlagNone;
    }

    if (flags & FlagWatching) {
      scheduleNode(sub);
    }

    if (flags & FlagMutable) {
      const subSubs = sub.subs;
      if (subSubs !== undefined) {
        const nextSub = (link = subSubs).nextSub;
        if (nextSub !== undefined) {
          stack.push(next);
          next = nextSub;
        }
        continue;
      }
    }

    if ((link = next!) !== undefined) {
      next = link.nextSub;
      continue;
    }

    while (stack.length > 0) {
      link = stack.pop()!;
      if (link !== undefined) {
        next = link.nextSub;
        continue top;
      }
    }

    break;
  }
}

export function checkDirty(link: Link, sub: ReactiveNode): boolean {
  // Only `Link` is ever pushed here (unlike `propagate`), so pops stay typed.
  const stack: Link[] = [];
  let checkDepth = 0;
  let dirty = false;

  top: for (;;) {
    const dep = link.dep;
    const flags = dep.flags;

    if (sub.flags & FlagDirty) {
      dirty = true;
    } else if ((flags & (FlagMutable | FlagDirty)) === (FlagMutable | FlagDirty)) {
      const subs = dep.subs!;
      if (dep.update!()) {
        if (subs.nextSub !== undefined) {
          shallowPropagate(subs);
        }
        dirty = true;
      }
    } else if ((flags & (FlagMutable | FlagPending)) === (FlagMutable | FlagPending)) {
      // Production assumes computed deps form a DAG; a cycle makes this descent loop forever.
      // Dev reports it and treats the cyclic edge as clean so the check terminates.
      if (process.env.NODE_ENV !== "production" && isOnCheckPath(dep, sub, stack)) {
        console.warn(
          "[rezejs] Cycle detected in computed dependencies. Computed graphs must be acyclic; " +
            "in production this does not terminate.",
        );
      } else {
        stack.push(link);
        link = dep.deps!;
        sub = dep;
        ++checkDepth;
        continue;
      }
    }

    if (!dirty) {
      const nextDep = link.nextDep;
      if (nextDep !== undefined) {
        link = nextDep;
        continue;
      }
    }

    while (checkDepth--) {
      link = stack.pop()!;
      if (dirty) {
        const subs = sub.subs!;
        if (sub.update!()) {
          if (subs.nextSub !== undefined) {
            shallowPropagate(subs);
          }
          sub = link.sub;
          continue;
        }
        dirty = false;
      } else {
        sub.flags &= ~FlagPending;
      }
      sub = link.sub;
      const nextDep = link.nextDep;
      if (nextDep !== undefined) {
        link = nextDep;
        continue top;
      }
    }

    return dirty && !!sub.flags;
  }
}

/** Dev only: whether `dep` is `sub` or one of the nodes `checkDirty` descended through to reach it. */
function isOnCheckPath(dep: ReactiveNode, sub: ReactiveNode, stack: Link[]): boolean {
  if (dep === sub) {
    return true;
  }
  for (const link of stack) {
    if (link.sub === dep) {
      return true;
    }
  }
  return false;
}

export function shallowPropagate(link: Link): void {
  do {
    const sub = link.sub;
    const flags = sub.flags;
    if ((flags & (FlagPending | FlagDirty)) === FlagPending) {
      sub.flags = flags | FlagDirty;
      if ((flags & (FlagWatching | FlagRecursedCheck)) === FlagWatching) {
        scheduleNode(sub);
      }
    }
  } while ((link = link.nextSub!) !== undefined);
}

function isValidLink(checkLink: Link, sub: ReactiveNode): boolean {
  let link = sub.depsTail;
  while (link !== undefined) {
    if (link === checkLink) {
      return true;
    }
    link = link.prevDep;
  }
  return false;
}
