import type { JSX } from "./jsx";

type Falsy = false | 0 | "" | null | undefined;

export interface ShowProps<T> {
  when: T | Falsy;
  fallback?: JSX.Element;
  children: JSX.Element | ((value: () => T) => JSX.Element);
}

export interface ForProps<T> {
  each: readonly T[] | null | undefined | false;
  fallback?: JSX.Element;
  keyed?: true;
  children: (item: T, index: () => number) => JSX.Element;
}

export interface ForIndexProps<T> {
  each: readonly T[] | null | undefined | false;
  fallback?: JSX.Element;
  keyed: false;
  children: (item: () => T, index: number) => JSX.Element;
}

export interface ForKeyedProps<T> {
  each: readonly T[] | null | undefined | false;
  fallback?: JSX.Element;
  /**
   * Rows follow `keyed(item)` across evaluations; a kept row keeps its nodes and takes the new
   * item in place, so a key must keep pointing at its own row.
   */
  keyed: (item: T) => unknown;
  children: (item: () => T, index: () => number) => JSX.Element;
}

export interface RepeatProps {
  count: number;
  fallback?: JSX.Element;
  children: (index: number) => JSX.Element;
}

export interface SwitchProps {
  fallback?: JSX.Element;
  children: JSX.Element;
}

export interface MatchProps<T> {
  when: T | Falsy;
  children: JSX.Element | ((value: () => T) => JSX.Element);
}

export interface LoadingProps {
  fallback?: JSX.Element;
  children: JSX.Element;
}

export interface ErroredProps {
  /** A function written in place as the attribute receives the error and `reset`, which builds the children again; any other value is shown as is. */
  fallback?: JSX.Element | ((error: unknown, reset: () => void) => JSX.Element);
  children: JSX.Element;
}

export interface PortalProps {
  /** Where the children are kept, the document body when absent or `null`; the same nodes move when it changes. */
  mount?: Node | null;
  children: JSX.Element;
}
