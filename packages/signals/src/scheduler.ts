// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { FlagRecursed, FlagWatching } from "./flags";
import type { ReactiveNode } from "./graph";
import { getActiveScope, liveScopes, scopeOfNode, type ExecutionScope } from "./internal/scope";

let notifyIndex = 0;
let queuedLength = 0;
const queued: (ReactiveNode | undefined)[] = [];
let isFlushScheduled = false;

function flushGlobal(): void {
  if (notifyIndex >= queuedLength) {
    return;
  }
  try {
    while (notifyIndex < queuedLength) {
      const node = queued[notifyIndex]!;
      queued[notifyIndex++] = undefined;
      node.run!();
    }
  } finally {
    while (notifyIndex < queuedLength) {
      const node = queued[notifyIndex]!;
      queued[notifyIndex++] = undefined;
      node.flags |= FlagWatching | FlagRecursed;
    }
    notifyIndex = 0;
    queuedLength = 0;
  }
}

/** Runs every queued subscriber now, including those queued while it runs. */
export function flush(): void {
  if (__REZE_HTML__) {
    const active = getActiveScope();
    if (active !== undefined && !active.disposed && !active.usesNormalScheduling()) {
      active.flush("inline");
      return;
    }
    for (const scope of liveScopes()) {
      if (!scope.disposed && !scope.usesNormalScheduling()) {
        scope.flush("inline");
      }
    }
  }
  flushGlobal();
}

function scheduleGlobalFlush(): void {
  if (!isFlushScheduled) {
    isFlushScheduled = true;
    queueMicrotask(() => {
      isFlushScheduled = false;
      flushGlobal();
    });
  }
}

function scopedTarget(node: ReactiveNode): ExecutionScope | undefined {
  const scope = scopeOfNode(node);
  if (scope !== undefined && !scope.disposed && !scope.usesNormalScheduling()) {
    return scope;
  }
  return undefined;
}

/**
 * Defers {@link flush} to a microtask, coalescing every write in this task into one run of each
 * subscriber. Signal values still update synchronously on write.
 */
export function scheduleFlush(): void {
  if (__REZE_HTML__) {
    const active = getActiveScope();
    if (active !== undefined && !active.disposed && !active.usesNormalScheduling()) {
      active.scheduleFlush();
      return;
    }
  }
  scheduleGlobalFlush();
}

function pushGlobal(node: ReactiveNode): void {
  queued[queuedLength++] = node;
}

function enqueueGlobal(node: ReactiveNode): void {
  let insertIndex = queuedLength;
  let firstInsertedIndex = insertIndex;
  let next: ReactiveNode | undefined = node;

  do {
    queued[insertIndex++] = next;
    next.flags &= ~FlagWatching;
    next = next.subs?.sub;
  } while (next !== undefined && next.flags & FlagWatching);

  queuedLength = insertIndex;

  while (firstInsertedIndex < --insertIndex) {
    const outer = queued[firstInsertedIndex];
    queued[firstInsertedIndex++] = queued[insertIndex];
    queued[insertIndex] = outer;
  }
}

/** Queues a watching node and its watching owners, outermost first. */
export function scheduleNode(node: ReactiveNode): void {
  if (!__REZE_HTML__) {
    enqueueGlobal(node);
    return;
  }
  const chain: ReactiveNode[] = [];
  let next: ReactiveNode | undefined = node;
  do {
    chain.push(next);
    next.flags &= ~FlagWatching;
    next = next.subs?.sub;
  } while (next !== undefined && next.flags & FlagWatching);
  let globalTouched = false;
  let touched: ExecutionScope[] | undefined;
  for (let i = chain.length - 1; i >= 0; i--) {
    const queuedNode = chain[i]!;
    const target = scopedTarget(queuedNode);
    if (target === undefined) {
      pushGlobal(queuedNode);
      globalTouched = true;
    } else {
      target.pushNode(queuedNode);
      if (touched === undefined) {
        touched = [target];
      } else if (!touched.includes(target)) {
        touched.push(target);
      }
    }
  }
  if (touched !== undefined) {
    for (const target of touched) {
      target.scheduleFlush();
    }
  }
  if (globalTouched) {
    scheduleGlobalFlush();
  }
}
