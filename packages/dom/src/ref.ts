import { untrack } from "@rezejs/signals";

/** Calls `fn(el, arg)` untracked, so a ref callback never subscribes its caller. */
export function use<E extends Element, T>(fn: (el: E, arg?: T) => void, el: E, arg?: T): void {
  untrack(() => fn(el, arg));
}
