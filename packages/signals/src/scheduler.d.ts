import type { ReactiveNode } from "./graph";
/** Runs every queued subscriber now, including those queued while it runs. */
export declare function flush(): void;
/**
 * Defers {@link flush} to a microtask, coalescing every write in this task into one run of each
 * subscriber. Signal values still update synchronously on write.
 */
export declare function scheduleFlush(): void;
/** Queues a watching node and its watching owners, outermost first. */
export declare function scheduleNode(node: ReactiveNode): void;
