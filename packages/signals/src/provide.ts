import { adopt, getOwner, parentOwner, runWithOwner } from "./context";
import { FlagNone } from "./flags";
import { disposeNode, type Link, type ReactiveNode } from "./graph";

/** A context's identity and the value `useContext` returns where nothing provides it. */
export interface ContextKey<T> {
  readonly id: symbol;
  readonly defaultValue: T;
}

class ProviderNode implements ReactiveNode {
  deps: Link | undefined = undefined;
  depsTail: Link | undefined = undefined;
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  flags: number = FlagNone;
  contextId: symbol;
  value: unknown;

  constructor(contextId: symbol, value: unknown) {
    this.contextId = contextId;
    this.value = value;
  }

  dispose(): void {
    disposeNode(this);
  }

  unwatched(): void {
    this.dispose();
  }
}

/**
 * Runs `fn` untracked under a new owner that provides `value` for `context`: `useContext` in
 * anything created inside, now or later, sees it. The owner lives as long as the current one.
 */
export function provideContext<T, R>(context: ContextKey<T>, value: T, fn: () => R): R {
  const node = new ProviderNode(context.id, value);
  const owner = getOwner();
  if (owner !== undefined) {
    adopt(node, owner);
  }
  return runWithOwner(node, fn);
}

/** The value the nearest enclosing provider of `context` gives, else its default. */
export function useContext<T>(context: ContextKey<T>): T {
  for (let owner = getOwner(); owner !== undefined; owner = parentOwner(owner)) {
    if (owner instanceof ProviderNode && owner.contextId === context.id) {
      return owner.value as T;
    }
  }
  return context.defaultValue;
}
