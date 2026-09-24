import type { ReactiveNode } from "./graph";

export type DebugNodeKind = "signal" | "computed" | "effect" | "render" | "root";

/**
 * What a devtools registry hears from the runtime. Called only in development builds
 * (`process.env.NODE_ENV !== "production"`), and only while installed.
 */
export interface DebugHook {
  /** `node` was created under the current owner; `name` is the `name` option when given. */
  created(
    node: ReactiveNode,
    kind: DebugNodeKind,
    name: string | undefined,
    read: () => unknown,
  ): void;
  /** `node` is about to re-run: what it created in its previous run is gone. */
  rerunning(node: ReactiveNode): void;
  disposed(node: ReactiveNode): void;
  /** A signal was written a value different from its previous one. */
  written(node: ReactiveNode): void;
  /** Runs a component's body, with everything it creates grouped under `name`. */
  component<T>(name: string, run: () => T): T;
}

export let debugHook: DebugHook | undefined;

/** Installs the development registry `@rezejs/devtools` builds; `undefined` removes it. */
export function setDebugHook(hook: DebugHook | undefined): void {
  debugHook = hook;
}
