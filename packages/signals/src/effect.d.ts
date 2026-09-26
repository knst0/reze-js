type EffectCleanup = (() => void) | void;
/**
 * Runs `fn` now and again, on the next flush, whenever what it read changes. The function `fn`
 * returns runs before the next run and on disposal. Nested nodes are disposed first, newest first.
 * Returns the disposer.
 */
export declare function effect(fn: () => EffectCleanup): () => void;
export {};
