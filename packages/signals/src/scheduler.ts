// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { FlagRecursed, FlagWatching } from "./flags";
import type { ReactiveNode } from "./graph";

let notifyIndex = 0;
let queuedLength = 0;
const queued: (ReactiveNode | undefined)[] = [];
let isFlushScheduled = false;

/** Runs every queued subscriber now, including those queued while it runs. */
export function flush(): void {
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

function flushScheduled(): void {
  isFlushScheduled = false;
  flush();
}

/**
 * Defers {@link flush} to a microtask, coalescing every write in this task into one run of each
 * subscriber. Signal values still update synchronously on write.
 */
export function scheduleFlush(): void {
  if (!isFlushScheduled) {
    isFlushScheduled = true;
    queueMicrotask(flushScheduled);
  }
}

/** Queues a watching node and its watching owners, outermost first. */
export function scheduleNode(node: ReactiveNode): void {
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
