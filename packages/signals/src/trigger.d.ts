/** Runs `fn`, then notifies the subscribers of every dependency it read and flushes. */
export declare function trigger(fn: () => void): void;
