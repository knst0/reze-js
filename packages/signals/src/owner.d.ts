import { type ReactiveNode } from "./graph";
/** Opaque handle to a node that owns computations (`root`, `effect`, `computed`, render bindings). */
export type Owner = ReactiveNode;
/**
 * Runs `fn` untracked in a new owner detached from the current one. Everything created inside
 * lives until `dispose` is called; it still sees the current owner's context.
 */
export declare function root<T>(fn: (dispose: () => void) => T): T;
/**
 * Registers `fn` to run, untracked, when the current owner re-runs or is disposed; cleanups and
 * owned nodes are released newest first. No-op without an owner.
 */
export declare function onCleanup(fn: () => void): void;
/** Runs `fn` without tracking reads; computations created inside stay owned by the current owner. */
export declare function untrack<T>(fn: () => T): T;
