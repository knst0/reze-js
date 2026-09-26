/**
 * A deep reactive object: `state` reads as if every own property of every object and array in
 * the tree were a `signal` (plus one signal per key set), and throws `TypeError` on writes.
 * `setState(fn)` runs `fn(draft)` untracked; writes through `draft` notify immediately, and
 * `draft` proxies throw `TypeError` once `fn` returns. `init` is adopted, not copied.
 */
export declare function store<T extends object>(init: T): [state: T, setState: (fn: (draft: T) => void) => void];
