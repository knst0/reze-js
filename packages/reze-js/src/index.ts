import type { JSX } from "./jsx-runtime";
import type { Loader } from "./runtime/server";

export type { JSX };

/** The app's routes by pattern; the generated `routes.gen.d.ts` fills it. */
// oxlint-disable-next-line no-empty-interface
export interface Register {}

interface RouteEntry {
  params: Record<string, string | undefined>;
  data: unknown;
}

type Routes = Register extends { routes: infer R } ? R : Record<string, RouteEntry>;
type Layouts = Register extends { layouts: infer L } ? L : Record<string, RouteEntry>;

/** The data of a route module's `load`. */
export type RouteData<M> = M extends { load: Loader<infer T> } ? T : undefined;
/** Props of the page at `pattern`: `params` and `data`. */
export type RoutePropsFor<P extends keyof Routes> = Routes[P];
/** Props of the layout that wraps `prefix`: `params`, `data`, and `children`. */
export type LayoutPropsFor<P extends keyof Layouts> = Layouts[P] & { children: JSX.Element };

type Branch<T> = JSX.Element | ((value: NonNullable<T>) => JSX.Element);

/** Compiler syntax for state: reads and writes stay plain JS. */
export declare function signal<T>(): T | undefined;
export declare function signal<T>(initial: T): T;
/** Compiler syntax for a value derived from state. */
export declare function computed<T>(value: T): T;
/** Compiler syntax for an object or array whose fields are state. */
export declare function store<T extends object>(initial: T): T;
/** Compiler syntax for a function that writes state. */
export declare function action<F extends (...args: never[]) => unknown>(fn: F): F;
export declare function effect(fn: () => void): void;
export declare function onCleanup(fn: () => void): void;
export declare function untrack<T>(fn: () => T): T;
export declare function createUniqueId(): string;
export declare function useParams<T extends Record<string, string | undefined>>(): T;

export declare function Show<T>(props: { when: T; fallback?: JSX.Element; children: Branch<T> }): JSX.Element;
export declare function Switch(props: { fallback?: JSX.Element; children: JSX.Element }): JSX.Element;
export declare function Match<T>(props: { when: T; children: Branch<T> }): JSX.Element;
export declare function For<T>(props: {
  each: readonly T[] | null | undefined;
  key?: (T extends object ? keyof T & string : never) | ((item: T) => unknown);
  fallback?: JSX.Element;
  children: (item: T, index: number) => JSX.Element;
}): JSX.Element;
export declare function Repeat(props: { count: number; fallback?: JSX.Element; children: (index: number) => JSX.Element }): JSX.Element;
export declare function Loading(props: { fallback?: JSX.Element; children: JSX.Element }): JSX.Element;
/** What an `<Errored>` fallback receives: a route error on the server, a handler error in the browser. */
export interface ErrorInfo {
  status: number;
  message: string;
}
export declare function Errored(props: {
  fallback?: JSX.Element | ((error: ErrorInfo) => JSX.Element);
  children: JSX.Element;
}): JSX.Element;
export declare function Portal(props: { mount?: Element; children: JSX.Element }): JSX.Element;
export declare function dynamic<P>(component: (props: P) => JSX.Element): (props: P) => JSX.Element;

export interface Context<T> {
  readonly id: symbol;
  readonly defaultValue: T;
}

export function createContext<T>(defaultValue: T): Context<T> {
  return { id: Symbol(), defaultValue };
}

export declare function useContext<T>(context: Context<T>): T;
