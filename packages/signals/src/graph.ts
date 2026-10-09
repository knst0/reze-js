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
 * `createReactiveSystem`, so this module references no concrete node kind and a
 * bundle only contains the kinds it constructs. Traversal forks live in
 * function-local arrays allocated on the first fork only.
 */

import { FlagDirty, FlagMutable, FlagNone, FlagPending, FlagRecursed, FlagRecursedCheck, FlagWatching } from "./flags";
import { notifyNodeDisposed } from "./internal/scope";
import { profileDisposed } from "./profile";
import { scheduleNode } from "./scheduler";

export interface ReactiveNode {
  deps?: Link;
  depsTail?: Link;
  subs?: Link;
  subsTail?: Link;
  flags: number;
  /** Required on `FlagMutable` nodes: settles pending state and returns whether the value changed. */
  update?(): boolean;
  /** Required on `FlagWatching` nodes: executed by `flush` after being queued. */
  run?(): void;
  /** Called when the last subscriber unlinks. */
  unwatched?(): void;
  /** Present on owned nodes: they are unlinked from their owner before it re-runs. */
  dispose?(): void;
  /** The owner a root or computed was created under; it does not dispose the node. */
  parent?: ReactiveNode | undefined;
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

/** Unlinks the owned nodes of `sub`, newest first; each disposes itself on unlink. */
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

/** Detaches `node` from its deps, newest first, and from its owner. */
export function disposeNode(node: ReactiveNode): void {
  if (process.env.NODE_ENV !== "production") {
    profileDisposed(node);
  }
  node.flags = FlagNone;
  disposeAllDepsInReverse(node);
  const sub = node.subs;
  if (sub !== undefined) {
    unlink(sub);
  }
  if (__REZE_HTML__) {
    notifyNodeDisposed(node);
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

/** Unlinks the deps `sub` did not re-read in its latest run. */
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

/** Marks every transitive subscriber of `link.dep` pending and queues the watching ones. */
export function propagate(link: Link, isInnerWrite: boolean): void {
  let next = link.nextSub;
  let forks: (Link | undefined)[] | undefined;

  top: for (;;) {
    const sub = link.sub;
    let flags = sub.flags;

    if (!(flags & (FlagRecursedCheck | FlagRecursed | FlagDirty | FlagPending))) {
      sub.flags = isInnerWrite ? flags | FlagPending | FlagRecursed : flags | FlagPending;
    } else if (!(flags & (FlagRecursedCheck | FlagRecursed))) {
      flags = FlagNone;
    } else if (!(flags & FlagRecursedCheck)) {
      sub.flags = (flags & ~FlagRecursed) | FlagPending;
    } else if (!(flags & (FlagDirty | FlagPending)) && isValidLink(link, sub)) {
      sub.flags = flags | FlagRecursed | FlagPending;
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
          (forks ??= []).push(next);
          next = nextSub;
        }
        continue;
      }
    }

    if ((link = next!) !== undefined) {
      next = link.nextSub;
      continue;
    }

    if (forks !== undefined) {
      while (forks.length > 0) {
        link = forks.pop()!;
        if (link !== undefined) {
          next = link.nextSub;
          continue top;
        }
      }
    }

    return;
  }
}

/**
 * Settles the pending deps of `sub`, starting at `link`, and returns whether one of them changed.
 * Re-entrant: `update` runs user getters that may call back into the graph.
 */
export function checkDirty(link: Link, sub: ReactiveNode): boolean {
  let descents: Link[] | undefined;
  let depth = 0;
  let isDirty = false;

  top: for (;;) {
    const dep = link.dep;
    const flags = dep.flags;

    if (sub.flags & FlagDirty) {
      isDirty = true;
    } else if ((flags & (FlagMutable | FlagDirty)) === (FlagMutable | FlagDirty)) {
      const subs = dep.subs!;
      if (dep.update!()) {
        if (subs.nextSub !== undefined) {
          shallowPropagate(subs);
        }
        isDirty = true;
      }
    } else if ((flags & (FlagMutable | FlagPending)) === (FlagMutable | FlagPending)) {
      if (process.env.NODE_ENV !== "production" && isOnCheckPath(dep, sub, descents)) {
        console.warn(
          "[rezejs] Cycle detected in computed dependencies. Computed graphs must be acyclic; " + "in production this does not terminate.",
        );
      } else {
        (descents ??= []).push(link);
        link = dep.deps!;
        sub = dep;
        ++depth;
        continue;
      }
    }

    if (!isDirty) {
      const nextDep = link.nextDep;
      if (nextDep !== undefined) {
        link = nextDep;
        continue;
      }
    }

    while (depth--) {
      link = descents!.pop()!;
      if (isDirty) {
        const subs = sub.subs!;
        if (sub.update!()) {
          if (subs.nextSub !== undefined) {
            shallowPropagate(subs);
          }
          sub = link.sub;
          continue;
        }
        isDirty = false;
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

    return isDirty && sub.flags !== FlagNone;
  }
}

function isOnCheckPath(dep: ReactiveNode, sub: ReactiveNode, descents: readonly Link[] | undefined): boolean {
  if (dep === sub) {
    return true;
  }
  if (descents !== undefined) {
    for (const descent of descents) {
      if (descent.sub === dep) {
        return true;
      }
    }
  }
  return false;
}

/** Marks the direct subscribers starting at `link` dirty and queues the watching ones. */
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
