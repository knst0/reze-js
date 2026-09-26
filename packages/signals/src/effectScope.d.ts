/**
 * Runs `fn` in a new owner that lives as long as the current one. Returns the disposer; reads in
 * `fn` are tracked by the scope and do not re-run it.
 */
export declare function effectScope(fn: () => void): () => void;
