/** The tracking context the compiler threads through an async computation. */
export interface AsyncContext {
    /**
     * Reads `source` as a dependency of this run, also after an `await`. Once a newer run started or
     * the computation was disposed, reads are untracked.
     */
    get<T>(source: () => T): T;
}
/** The state of an async computation; every getter is tracked. */
export interface AsyncComputed<T> {
    /** The latest resolved value; `undefined` until the first run resolves. Kept while a re-run is pending. */
    value(): T | undefined;
    /** Whether the latest run has not settled yet. */
    isPending(): boolean;
    /** The rejection of the latest run; cleared when a later run resolves. */
    error(): unknown;
}
/**
 * Runs `fn` now, and again on the next flush whenever a source it read synchronously or through
 * `c.get` changes. A settlement of a superseded run, or one after the owner disposed the
 * computation, is dropped. A rejection or a synchronous throw is stored in `error()`, not rethrown.
 */
export declare function asyncComputed<T>(fn: (c: AsyncContext) => PromiseLike<T> | T): AsyncComputed<T>;
