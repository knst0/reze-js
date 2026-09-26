export interface ComputedOptions {
    /** The name devtools show; ignored in production builds. */
    name?: string;
}
/**
 * A lazily evaluated, cached derivation. Recomputes on read after a dependency changed, and
 * notifies only when the result differs by `Object.is`. Nodes created in `getter` are owned by
 * it and disposed before each recomputation.
 */
export declare function computed<T>(getter: (previousValue?: T) => T, options?: ComputedOptions): () => T;
