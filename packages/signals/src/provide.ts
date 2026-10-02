import { adopt, getOwner, parentOwner, runWithOwner } from "./context";
import { FlagNone } from "./flags";
import { disposeNode, type Link, type ReactiveNode } from "./graph";

export class ContextNotFoundError extends Error {
  constructor() {
    super("[reze] useContext: no Provider for a default-less context");
    this.name = "ContextNotFoundError";
  }
}

/** A context's identity and the value `useContext` returns where nothing provides it. */
export interface ContextKey<T> {
  readonly id: symbol;
  readonly defaultValue: T;
}

/** A key that is also its own provider component: `<Ctx value={v}>children</Ctx>`. The `value` is read once. */
export interface Context<T> extends ContextKey<T> {
  readonly hasDefault: boolean;
  <C>(props: { value: T; children: C }): C;
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

/** The value the nearest enclosing provider of `context` gives, else its default. A default-less context with no provider throws `ContextNotFoundError`. */
export function useContext<T>(context: ContextKey<T>): T {
  for (let owner = getOwner(); owner !== undefined; owner = parentOwner(owner)) {
    if (owner instanceof ProviderNode && owner.contextId === context.id) {
      return owner.value as T;
    }
  }
  if ((context as Context<T>).hasDefault === false) {
    throw new ContextNotFoundError();
  }
  return context.defaultValue;
}

/**
 * A context that is its own provider: `useContext` in anything built inside `<Ctx value={v}>`
 * sees `v`. Without a default, reading outside a provider throws `ContextNotFoundError`.
 */
export function createContext<T>(): Context<T>;
export function createContext<T>(defaultValue: T): Context<T>;
export function createContext<T>(...args: [] | [defaultValue: T]): Context<T> {
  const key: ContextKey<T> = { id: Symbol(), defaultValue: args[0] as T };
  const provide = <C>(props: { value: T; children: C }): C => provideContext(key, props.value, () => props.children);
  return Object.assign(provide, { id: key.id, defaultValue: key.defaultValue, hasDefault: args.length > 0 });
}
