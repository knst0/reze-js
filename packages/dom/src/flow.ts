import {
  catchError,
  computed,
  onCleanup,
  root,
  signal,
  untrack,
  type Getter,
} from "@rezejs/signals";
import { renderEffect as bind } from "@rezejs/signals/render";

import { insertExpression, splitProps, spread } from "./dom";
import type { JSX } from "./jsx";

type Falsy = false | 0 | "" | null | undefined;
type Branch<T> = JSX.Element | ((value: Getter<T>) => JSX.Element);

/**
 * Builds a branch untracked: a function child taking an argument receives `value` as a getter.
 * Runs inside a `computed`, which owns the branch and disposes it when the branch switches.
 */
function renderBranch<T>(children: Branch<T>, value: Getter<T>): JSX.Element {
  return untrack(() =>
    typeof children === "function" && children.length
      ? (children as (value: Getter<T>) => JSX.Element)(value)
      : (children as JSX.Element),
  );
}

type KeyedBranch<T> = JSX.Element | ((value: T) => JSX.Element);

/** Builds a keyed branch untracked: a function child receives the value itself. */
function renderKeyedBranch<T>(children: KeyedBranch<T>, value: T): JSX.Element {
  return untrack(() =>
    typeof children === "function" && children.length
      ? (children as (value: T) => JSX.Element)(value)
      : (children as JSX.Element),
  );
}

export type ShowProps<T> =
  | { when: T | Falsy; keyed?: false; fallback?: JSX.Element; children: Branch<T> }
  | { when: T | Falsy; keyed: true; fallback?: JSX.Element; children: KeyedBranch<T> };

/**
 * Renders `children` while `when` is truthy. The branch is rebuilt only when truthiness flips.
 * The value getter a function child receives is a memo created on its first read.
 */
/**
 * With `keyed`, the branch is rebuilt whenever `when` changes identity, and a function child
 * receives the value itself.
 */
export function Show<T>(props: ShowProps<T>): JSX.Element {
  if (props.keyed) {
    const keyedProps = props;
    const when = computed(() => keyedProps.when);
    return computed(() => {
      const value = when();
      return value
        ? renderKeyedBranch(keyedProps.children, value as T)
        : untrack(() => keyedProps.fallback);
    });
  }
  const shown = computed(() => !!props.when);
  let when: Getter<T | Falsy> | undefined;
  const value = (): T => (when ??= computed(() => props.when))() as T;
  return computed(() =>
    shown() ? renderBranch(props.children as Branch<T>, value) : untrack(() => props.fallback),
  );
}

export interface ErroredProps {
  /** Shown instead of the children after they throw; a function gets the error and `reset`. */
  fallback: JSX.Element | ((error: Getter<unknown>, reset: () => void) => JSX.Element);
  children: JSX.Element;
}

/** `children` after running every getter in it once, so what they throw surfaces here. */
function evaluated(children: JSX.Element): JSX.Element {
  let value: unknown = children;
  while (typeof value === "function") value = value();
  if (Array.isArray(value)) value.forEach(evaluated);
  return children;
}

/**
 * Renders `children`, or `fallback` once they throw: while being created, or later from an
 * effect or binding inside them. `reset()` rebuilds the children. A nested `Errored` catches
 * first; an error the fallback throws reaches the enclosing one.
 */
export function Errored(props: ErroredProps): JSX.Element {
  const [attempt, setAttempt] = signal(0);
  const [error, setError] = signal<unknown>(undefined, { equals: false });
  let isFailed = false;
  const retry = (): void => void setAttempt((n) => n + 1);
  const reset = (): void => {
    isFailed = false;
    retry();
  };
  return computed(() => {
    attempt();
    if (!isFailed) {
      let isCreating = true;
      let dispose!: () => void;
      const children = root((disposeAttempt) => {
        dispose = disposeAttempt;
        return catchError(
          () => evaluated(props.children),
          (thrown) => {
            isFailed = true;
            setError(thrown);
            if (!isCreating) retry();
          },
        );
      });
      isCreating = false;
      if (!isFailed) {
        onCleanup(dispose);
        return children;
      }
      dispose();
    }
    return untrack(() => {
      const fallback = props.fallback;
      return typeof fallback === "function" ? fallback(error, reset) : fallback;
    });
  });
}

export type MatchProps<T> =
  | { when: T | Falsy; keyed?: false; children: Branch<T> }
  | { when: T | Falsy; keyed: true; children: KeyedBranch<T> };

/** A `<Switch>` case; evaluates to its own props, which `<Switch>` reads. */
export function Match<T>(props: MatchProps<T>): JSX.Element {
  return props as unknown as JSX.Element;
}

export interface SwitchProps {
  fallback?: JSX.Element;
  children: JSX.Element;
}

/**
 * Renders the first `<Match>` whose `when` is truthy; rebuilt only when that choice changes, or,
 * for a `keyed` match, when its `when` changes identity.
 */
export function Switch(props: SwitchProps): JSX.Element {
  const matches = computed(() => {
    const children = props.children;
    return (Array.isArray(children) ? children : [children]) as unknown as MatchProps<unknown>[];
  });
  const index = computed(() => matches().findIndex((m) => m.when));
  return computed(() => {
    const i = index();
    if (i < 0) return untrack(() => props.fallback);
    const match = matches()[i]!;
    if (match.keyed) return renderKeyedBranch(match.children, match.when);
    return renderBranch(match.children as Branch<unknown>, () => match.when);
  });
}

export type DynamicProps = {
  component: string | ((props: Record<string, unknown>) => JSX.Element) | false | null | undefined;
  [prop: string]: unknown;
};

/** Renders `component` (a tag name or a component) with the remaining props. */
export function Dynamic(props: DynamicProps): JSX.Element {
  const others = splitProps(props, ["component"])[1]!;
  const component = computed(() => props.component);
  return computed(() => {
    const c = component();
    if (!c) return;
    return untrack(() => {
      if (typeof c === "function") return c(others);
      const el = document.createElement(c);
      spread(el, others);
      return el;
    });
  });
}

export interface PortalProps {
  /** Defaults to `document.body`; read once. */
  mount?: Node;
  children: JSX.Element;
}

/** Renders `children` at the end of `mount` instead of in place; removed with its owner. */
export function Portal(props: PortalProps): JSX.Element {
  const mount = props.mount ?? document.body;
  const marker = mount.appendChild(document.createTextNode(""));
  let current: unknown = [];
  bind(() => {
    current = insertExpression(mount, props.children, current, marker);
  });
  onCleanup(() => {
    while (typeof current === "function") current = current();
    for (const node of current as ChildNode[]) node.remove();
    marker.remove();
  });
  return null;
}
