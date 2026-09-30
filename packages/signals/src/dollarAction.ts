import type { Action, ActionOptions } from "./action";

/**
 * Compiler syntax for an action: `const save = $action(async (todo) => { … })`. Store writes in
 * the body, also after each `await`, are speculative: visible at once, undone if the call fails.
 * Writes in nested functions run outside the action. Compiles to `action`.
 */
export function $action<Args extends unknown[], R>(fn: (...args: Args) => R | PromiseLike<R>, options?: ActionOptions): Action<Args, R>;
export function $action(): never {
  throw new Error("`$action()` requires the reze compiler");
}
