import { computed, getOwner, runWithOwner, signal, untrack } from "@rezejs/signals";

import { createComponent } from "./dom";
import { nextHydrationKey } from "./hydration";
import type { JSX } from "./jsx";
import { trackPending } from "./loading";

// oxlint-disable-next-line typescript/no-explicit-any
type Component = (props: any) => JSX.Element;

export type LazyComponent<C extends Component, M> = C & {
  /** Loads the module now; later renders use it without waiting. */
  preload: () => Promise<M>;
};

/**
 * A component whose code loads on first render (or `preload()`): until the module arrives the
 * nearest `Loading` shows its fallback, then `options.export` of the module (default
 * `"default"`) renders with the props. A failed load throws into the nearest `Errored`. For
 * server rendering and hydration, `await preload()` first: a component not loaded yet renders
 * nothing there.
 */
export function lazy<M extends Record<string, unknown>, K extends keyof M & string>(
  load: () => Promise<M>,
  options: { export: K },
): LazyComponent<M[K] & Component, M>;
export function lazy<C extends Component>(
  load: () => Promise<{ default: C }>,
  options?: { export?: "default" },
): LazyComponent<C, { default: C }>;
export function lazy(
  load: () => Promise<Record<string, unknown>>,
  options?: { export?: string },
): LazyComponent<Component, Record<string, unknown>> {
  const name = options?.export ?? "default";
  let loaded: Component | undefined;
  let loading: Promise<Record<string, unknown>> | undefined;
  const preload = (): Promise<Record<string, unknown>> =>
    (loading ??= load().then((module) => {
      const component = module[name];
      if (typeof component !== "function") {
        throw new Error(`lazy: the module has no component export "${name}"`);
      }
      loaded = component as Component;
      return module;
    }));
  const Lazy = (props: Record<string, unknown>): JSX.Element => {
    if (loaded !== undefined) return createComponent(loaded, props);
    const [component, setComponent] = signal<Component | undefined>(undefined);
    const [error, setError] = signal<unknown>(undefined);
    let hasFailed = false;
    trackPending(() => component() === undefined && !hasFailed);
    const owner = getOwner();
    preload().then(
      () => runWithOwner(owner, () => setComponent(() => loaded)),
      (reason: unknown) =>
        runWithOwner(owner, () => {
          hasFailed = true;
          setError(() => reason);
        }),
    );
    return computed(() => {
      const failure = error();
      if (hasFailed) throw failure;
      const ready = component();
      return ready === undefined ? undefined : untrack(() => createComponent(ready, props));
    });
  };
  return Object.assign(Lazy, { preload });
}

let clientIds = 0;

/**
 * An id unique in the document, the same on the server and in the client that hydrates its
 * output: take it in the component body, in the same order on both sides.
 */
export function createUniqueId(): string {
  const key = nextHydrationKey();
  return key === undefined ? `cl-${clientIds++}` : `hk-${key}`;
}
