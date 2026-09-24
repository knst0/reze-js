import { provideContext, type ContextKey } from "@rezejs/signals";

import type { JSX } from "./jsx";

/** `<Context value={v}>children</Context>` provides `v` to `useContext(Context)` below it. */
export interface Context<T> extends ContextKey<T> {
  (props: { value: T; children?: JSX.Element }): JSX.Element;
}

/**
 * A context and its provider component. `value` is read once, when the provider renders: pass
 * a getter or a store to share reactive state.
 */
export function createContext<T>(defaultValue: T): Context<T>;
export function createContext<T>(): Context<T | undefined>;
export function createContext<T>(defaultValue?: T): Context<T | undefined> {
  const provider = ((props: { value: T; children?: JSX.Element }) =>
    provideContext(context, props.value, () => props.children)) as Context<T | undefined>;
  const context = Object.assign(provider, { id: Symbol("context"), defaultValue });
  return context;
}
