import { type ReactiveNode } from "./graph";
/** The subscriber that reads right now are tracked into. */
export declare let activeSub: ReactiveNode | undefined;
/** Number of subscriber bodies executing; a write inside one is an inner write. */
export declare let effectDepth: number;
export declare function setActiveSub(sub: ReactiveNode | undefined): ReactiveNode | undefined;
export declare function setActiveOwner(owner: ReactiveNode | undefined): ReactiveNode | undefined;
/** The node that owns computations created right now, if any. */
export declare function getOwner(): ReactiveNode | undefined;
/** Records `node` as owned by `owner`: disposed before the owner re-runs and when it is disposed. */
export declare function adopt(node: ReactiveNode, owner: ReactiveNode): void;
/** Makes `sub` active and records it as owned by the current owner. */
export declare function enterOwner(sub: ReactiveNode): ReactiveNode | undefined;
/**
 * Runs `fn` untracked under `owner`, so nodes created inside are owned by it; for work resumed
 * after an `await`.
 */
export declare function runWithOwner<T>(owner: ReactiveNode | undefined, fn: () => T): T;
export declare function enterEffect(): void;
export declare function exitEffect(): void;
/**
 * The owner `node` was created under: the recorded `parent` of a root or computed, else the owner
 * an adopted node is linked to.
 */
export declare function parentOwner(node: ReactiveNode): ReactiveNode | undefined;
/** Installs where errors thrown by computations go (`catchError`); it returns whether it handled one. */
export declare function setErrorHook(hook: (node: ReactiveNode, error: unknown) => boolean): void;
/** Whether the error hook handled an error `node` threw. */
export declare function isErrorHandled(node: ReactiveNode, error: unknown): boolean;
/** Hands an error a scheduled run of `node` threw to the error hook, or rethrows it. */
export declare function reportError(node: ReactiveNode, error: unknown): void;
/** Starts a tracked re-run of `sub`; pair with `endTracking` in a `finally`. */
export declare function startTracking(sub: ReactiveNode, flags: number): ReactiveNode | undefined;
export declare function endTracking(sub: ReactiveNode, prevSub: ReactiveNode | undefined): void;
/** Subscribes the active sub, if any, to `dep`. */
export declare function track(dep: ReactiveNode): void;
