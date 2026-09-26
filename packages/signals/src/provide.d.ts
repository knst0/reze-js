/** A context's identity and the value `useContext` returns where nothing provides it. */
export interface ContextKey<T> {
    readonly id: symbol;
    readonly defaultValue: T;
}
/**
 * Runs `fn` untracked under a new owner that provides `value` for `context`: `useContext` in
 * anything created inside, now or later, sees it. The owner lives as long as the current one.
 */
export declare function provideContext<T, R>(context: ContextKey<T>, value: T, fn: () => R): R;
/** The value the nearest enclosing provider of `context` gives, else its default. */
export declare function useContext<T>(context: ContextKey<T>): T;
