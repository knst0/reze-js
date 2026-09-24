// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { FlagHasChildEffect, FlagRecursedCheck } from "./flags";
import { link, purgeDeps, type ReactiveNode } from "./graph";

let activeSub: ReactiveNode | undefined;
/** Owner for nodes created while no sub is active: set by `untrack`, `root`, `runWithOwner`. */
let activeOwner: ReactiveNode | undefined;
/** Link-dedup version, bumped on every tracked re-run. */
let version = 0;
/** Number of effect bodies currently executing; writes inside one are inner writes. */
export let effectDepth = 0;

export function getActiveSub(): ReactiveNode | undefined {
  return activeSub;
}

export function setActiveSub(sub?: ReactiveNode): ReactiveNode | undefined {
  const prevSub = activeSub;
  activeSub = sub;
  return prevSub;
}

/** The node that owns computations created right now, if any. */
export function getOwner(): ReactiveNode | undefined {
  return activeSub ?? activeOwner;
}

export function setActiveOwner(owner?: ReactiveNode): ReactiveNode | undefined {
  const prevOwner = activeOwner;
  activeOwner = owner;
  return prevOwner;
}

/** Records `node` as owned by `owner`: disposed before the owner re-runs and when it is disposed. */
export function adopt(node: ReactiveNode, owner: ReactiveNode): void {
  link(node, owner, 0);
  owner.flags |= FlagHasChildEffect;
}

export function enterEffect(): void {
  ++effectDepth;
}

export function exitEffect(): void {
  --effectDepth;
}

/**
 * The owner `node` was created under: the recorded `parent` of a root or computed, else the
 * owner an adopted node is linked to.
 */
export function parentOwner(node: ReactiveNode): ReactiveNode | undefined {
  if (node.parent !== undefined) {
    return node.parent;
  }
  return node.dispose !== undefined ? node.subs?.sub : undefined;
}

/** The first defined `pick(owner)` from the current owner up through its ancestors. */
export function lookupOwner<T>(pick: (owner: ReactiveNode) => T | undefined): T | undefined {
  for (let owner = getOwner(); owner !== undefined; owner = parentOwner(owner)) {
    const found = pick(owner);
    if (found !== undefined) {
      return found;
    }
  }
  return undefined;
}

let errorHook: ((node: ReactiveNode, error: unknown) => boolean) | undefined;

/** Installs where errors thrown by computations go (`catchError`); it returns whether it handled one. */
export function setErrorHook(hook: (node: ReactiveNode, error: unknown) => boolean): void {
  errorHook = hook;
}

/** Whether the error hook handled an error `node` threw. */
export function isErrorHandled(node: ReactiveNode, error: unknown): boolean {
  return errorHook !== undefined && errorHook(node, error);
}

/** Hands an error a scheduled run of `node` threw to the error hook, or rethrows it. */
export function reportError(node: ReactiveNode, error: unknown): void {
  if (!isErrorHandled(node, error)) {
    throw error;
  }
}

/** Makes `sub` active and records it as owned by the current owner. */
export function enterOwner(sub: ReactiveNode): ReactiveNode | undefined {
  const prevSub = activeSub;
  const owner = prevSub ?? activeOwner;
  activeSub = sub;
  if (owner !== undefined) {
    adopt(sub, owner);
  }
  return prevSub;
}

/** Starts a tracked re-run of `sub`; pair with `endTracking` in a `finally`. */
export function startTracking(sub: ReactiveNode, flags: number): ReactiveNode | undefined {
  ++version;
  sub.depsTail = undefined;
  sub.flags = flags | FlagRecursedCheck;
  const prevSub = activeSub;
  activeSub = sub;
  return prevSub;
}

export function endTracking(sub: ReactiveNode, prevSub: ReactiveNode | undefined): void {
  activeSub = prevSub;
  sub.flags &= ~FlagRecursedCheck;
  purgeDeps(sub);
}

/** Subscribes the active sub, if any, to `dep`. */
export function track(dep: ReactiveNode): void {
  if (activeSub !== undefined) {
    link(dep, activeSub, version);
  }
}
