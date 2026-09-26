import { computed, signal, untrack, type Setter } from "@rezejs/signals";

import type { JSX } from "./jsx";

interface HotEntry {
  set: Setter<unknown>;
  proxy: unknown;
}

let registry: Map<string, HotEntry> | undefined;

/**
 * Returns `component` unless `hot` (the module's `import.meta.hot`) is set. Then returns one stable proxy per `id`:
 * registering `id` again re-renders every mounted proxy instance in place with the new `component`, resetting its state.
 */
export function hotComponent<P>(hot: unknown, id: string, component: (props: P) => JSX.Element): (props: P) => JSX.Element {
  if (!hot) {
    return component;
  }
  const entry = (registry ??= new Map()).get(id);
  if (entry !== undefined) {
    entry.set(() => component);
    return entry.proxy as (props: P) => JSX.Element;
  }
  const [current, set] = signal<unknown>(component, { equals: false });
  const proxy = (props: P): JSX.Element =>
    computed(() => {
      const Component = current() as (props: P) => JSX.Element;
      return untrack(() => Component(props));
    });
  registry.set(id, { set, proxy });
  return proxy;
}
