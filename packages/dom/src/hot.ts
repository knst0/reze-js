import { computed, createSignalFrame, runWithSignalFrame, signal, untrack, type Setter } from "@rezejs/signals";

import type { JSX } from "./jsx";

interface HotEntry {
  set: Setter<unknown>;
  proxy: unknown;
}

let registry: Map<string, HotEntry> | undefined;

/**
 * Returns `component` unless `hot` (the module's `import.meta.hot`) is set. Then returns one stable proxy per `id`:
 * registering `id` again re-renders every mounted proxy instance in place with the new `component`, preserving
 * signal state by declaration order. Keep signal declarations stable in order and type across edits: a changed
 * type at a position resets that position, and editing an initializer does not overwrite a preserved value.
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
  const proxy = (props: P): JSX.Element => {
    const frame = createSignalFrame();
    return computed(() => {
      const Component = current() as (props: P) => JSX.Element;
      return untrack(() => runWithSignalFrame(frame, () => Component(props)));
    });
  };
  registry.set(id, { set, proxy });
  return proxy;
}
