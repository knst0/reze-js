import type {
  ClassValue,
  ForIndexProps,
  ForKeyedProps,
  ForProps,
  IslandOptions,
  IslandTrigger,
  JSX,
  MatchProps,
  PortalProps,
  Props,
  PropsOf,
  RepeatProps,
  ShowProps,
  SplitProps,
  SwitchProps,
} from "@rezejs/dom";
import type {
  Action,
  ActionOptions,
  ComputedOptions,
  Context,
  ContextKey,
  Equals,
  Getter,
  Owner,
  Run,
  Setter,
  SignalOptions,
} from "@rezejs/signals";

export type {
  Action,
  ActionOptions,
  ClassValue,
  ComputedOptions,
  Context,
  ContextKey,
  Equals,
  ForIndexProps,
  ForKeyedProps,
  ForProps,
  Getter,
  IslandOptions,
  IslandTrigger,
  JSX,
  MatchProps,
  Owner,
  PortalProps,
  Props,
  PropsOf,
  RepeatProps,
  Run,
  Setter,
  ShowProps,
  SignalOptions,
  SplitProps,
  SwitchProps,
};

export { hydrate, render } from "@rezejs/dom";

/**
 * Compiler syntax for a reactive variable: `let count = signal(0)`. The compiler turns reads
 * into getter calls and writes into setter calls. Declare it with `let` or `const` in the file
 * that uses it; it cannot be exported, destructured or passed around.
 */
export declare function signal<T>(): T | undefined;
export declare function signal<T>(initial: T, options?: SignalOptions<T>): T;

/**
 * Compiler syntax for a derived variable: `const doubled = computed(count * 2)`. The compiler
 * wraps the expression into a lazy cached derivation and turns reads into getter calls. Pass the
 * expression itself, not a function. Declare it in the file that uses it; it cannot be written,
 * exported, destructured or passed around.
 */
export declare function computed<T>(value: T, options?: ComputedOptions): T;

/**
 * Compiler syntax in an async component's load: the signal of the run reading it. It aborts when a
 * newer run starts or the component is disposed, and reads as already aborted after either.
 */
export declare function abortSignal(): AbortSignal;

/**
 * Compiler syntax in an async component's view: whether a re-run of a load in this component or its
 * subtree is pending. It reports reloads only, not first loads.
 */
export declare function isPending(): boolean;

/**
 * Compiler syntax for an action: `const save = action(async (todo) => { … })`. Store writes in
 * the body, also after each `await`, are speculative: visible at once, undone if the call fails.
 * Writes in nested functions run outside the action.
 */
export declare function action<Args extends unknown[], R>(
  fn: (...args: Args) => R | PromiseLike<R>,
  options?: ActionOptions,
): Action<Args, R>;

/**
 * Merges sources left to right: the last defined value wins, `undefined` counts as absent.
 * Dissolves into an object literal when every source is an object literal of defined values
 * with static keys.
 */
export declare function mergeProps(...sources: unknown[]): Props;

/**
 * Splits `props` into lazy views, one per key group plus one with the remaining keys.
 * Dissolves into literals when `props` is an object literal and every group is an array
 * literal of strings.
 */
export declare function splitProps<T extends object, const K extends readonly (readonly (keyof T)[])[]>(
  props: T,
  ...groups: K
): SplitProps<T, K>;

/**
 * The rest of `props` without `keys`. Dissolves into a literal when `props` is an object
 * literal and every key is a string literal.
 */
export declare function omitProps(props: Props, ...keys: readonly PropertyKey[]): Props;

/**
 * A deep reactive object: reads track every own property of every plain object and array in the
 * tree as if it were a `signal`, and writes change the object in place and notify immediately.
 * `initial` is adopted, not copied.
 */
export declare function store<T extends object>(initial: T): T;

/**
 * Runs `fn` now and again, on the next flush, whenever what it read changes. The function `fn`
 * returns runs before the next run and on disposal.
 */
export declare function effect(fn: () => void): void;

/**
 * A context that is its own provider: `useContext` in anything built inside `<Ctx value={v}>`
 * sees `v`. Without a default, reading outside a provider throws `ContextNotFoundError`.
 */
export declare function createContext<T>(): Context<T>;
export declare function createContext<T>(defaultValue: T): Context<T>;

/**
 * Runs `fn` untracked under a new owner that provides `value` for `context`: `useContext` in
 * anything created inside, now or later, sees it. The owner lives as long as the current one.
 */
export declare function provideContext<T, R>(context: ContextKey<T>, value: T, fn: () => R): R;

/** The value the nearest enclosing provider of `context` gives, else its default. */
export declare function useContext<T>(context: ContextKey<T>): T;

export declare function Show<T>(props: ShowProps<T>): JSX.Element;

export declare function For<T>(props: ForProps<T>): JSX.Element;
export declare function For<T>(props: ForIndexProps<T>): JSX.Element;
export declare function For<T>(props: ForKeyedProps<T>): JSX.Element;

export declare function Repeat(props: RepeatProps): JSX.Element;

export declare function Switch(props: SwitchProps): JSX.Element;

export declare function Match<T>(props: MatchProps<T>): JSX.Element;

export declare function Portal(props: PortalProps): JSX.Element;

/**
 * A component that renders what `source()` returns: a component, or nothing when falsy. It switches when `source()`
 * changes to a different value, disposing what it rendered before; props stay live either way. Tag names work only as
 * string literals `source` returns, which the compiler turns into components; use `dynamicElement` for tag names chosen
 * at runtime.
 */
export declare function dynamic<T extends JSX.ElementType>(source: () => T | null | undefined | false): (props: PropsOf<T>) => JSX.Element;

/**
 * `dynamic` that also renders tag names chosen at runtime, such as `props.as`, in the namespace of the tag: SVG for SVG
 * element names, MathML for `math`, HTML otherwise.
 */
export declare function dynamicElement<T extends JSX.ElementType>(
  source: () => T | null | undefined | false,
): (props: PropsOf<T>) => JSX.Element;

/**
 * A component that renders `fallback` and loads the real one when `trigger` fires:
 * `eager` loads at once, `idle` on the first idle period, `visible` when the shell
 * scrolls into view, `media` when its query matches, `interaction` on the first
 * pointer, focus or key event inside the shell.
 */
export declare function island<P>(
  trigger: IslandTrigger,
  load: () => ((props: P) => JSX.Element) | PromiseLike<(props: P) => JSX.Element>,
  props: P,
  fallback?: () => JSX.Element,
  options?: IslandOptions,
): JSX.Element;

/** An id that is a valid CSS identifier and unique on the page. */
export declare function createUniqueId(): string;

/**
 * Runs `fn` untracked under an owner that catches errors: a throw from `fn` itself returns
 * `undefined`, and a throw from any effect, binding or computed created inside goes to `handler`
 * too. Nested `catchError`s catch first; errors a handler throws go outward.
 */
export declare function catchError<T>(fn: () => T, handler: (error: unknown) => void): T | undefined;

export declare class ContextNotFoundError extends Error {
  constructor();
}

/**
 * Runs `fn` in a new owner that lives as long as the current one. Returns the disposer; reads in
 * `fn` are tracked by the scope and do not re-run it.
 */
export declare function effectScope(fn: () => void): () => void;

/** Runs every queued subscriber now, including those queued while it runs. */
export declare function flush(): void;

/** The node that owns computations created right now, if any. */
export declare function getOwner(): Owner | undefined;

/**
 * Registers `fn` to run, untracked, when the current owner re-runs or is disposed; cleanups and
 * owned nodes are released newest first. No-op without an owner.
 */
export declare function onCleanup(fn: () => void): void;

/** A view of `state` that reads the same signals and throws `TypeError` on every write. */
export declare function readonly<T extends object>(state: T): T;

/**
 * Runs `fn` untracked in a new owner detached from the current one. Everything created inside
 * lives until `dispose` is called; it still sees the current owner's context.
 */
export declare function root<T>(fn: (dispose: () => void) => T): T;

/**
 * Runs `fn` untracked under `owner`, so nodes created inside are owned by it; for work resumed
 * after an `await`.
 */
export declare function runWithOwner<T>(owner: Owner | undefined, fn: () => T): T;

/**
 * Returns `isSelected(key)`, which is `equals(key, source())` (default `Object.is`) and
 * subscribes its caller to that key's result only: when `source` changes, only the callers
 * whose result flipped re-run. Lives as long as the current owner.
 */
export declare function selector<K>(source: Getter<K>, equals?: (key: K, value: K) => boolean): (key: K) => boolean;

/** Runs `fn`, then notifies the subscribers of every dependency it read and flushes. */
export declare function trigger(fn: () => void): void;

/**
 * Runs `fn` without tracking reads; computations created inside stay owned by the current owner.
 * `untrack(fn, arg)` calls `fn(arg)`, which saves the closure `untrack(() => fn(arg))` allocates.
 */
export declare function untrack<T>(fn: () => T): T;
export declare function untrack<T, A>(fn: (arg: A) => T, arg: A): T;
