/**
 * Runs `fn` untracked under an owner that catches errors: a throw from `fn` itself returns
 * `undefined`, and a throw from any effect, binding or computed created inside goes to `handler`
 * too (a computed then keeps its previous value). Nested `catchError`s catch first; errors a
 * handler throws go outward. Lives as long as the current owner.
 */
export declare function catchError<T>(fn: () => T, handler: (error: unknown) => void): T | undefined;
