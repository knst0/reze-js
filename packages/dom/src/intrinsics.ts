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
  key?: (item: T) => unknown;
  children: (item: () => T, index: () => number) => JSX.Element;
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

function compiledAway(): never {
  throw new Error("[reze] Show, For, Switch, Match, Loading and Errored are compiled by @rezejs/vite-plugin and cannot run as functions");
}

export function Show<T>(_props: ShowProps<T>): JSX.Element {
  return compiledAway();
}

export function For<T>(_props: ForProps<T>): JSX.Element {
  return compiledAway();
}

export function Switch(_props: SwitchProps): JSX.Element {
  return compiledAway();
}

export function Match<T>(_props: MatchProps<T>): JSX.Element {
  return compiledAway();
}

export function Loading(_props: LoadingProps): JSX.Element {
  return compiledAway();
}

export function Errored(_props: ErroredProps): JSX.Element {
  return compiledAway();
}
