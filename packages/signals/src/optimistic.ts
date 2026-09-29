import { computed, type ComputedOptions } from "./computed";
import { onCleanup } from "./owner";
import { type Getter, SignalNode } from "./signal";

/**
 * Stacks `apply` over the source, after every earlier layer; returns its idempotent remover. The
 * layer is also removed when `until` settles and when the current owner re-runs or is disposed.
 */
export type Layer<T> = (apply: (value: T) => T, until?: PromiseLike<unknown>) => () => void;

interface LayerEntry<T> {
  apply: (value: T) => T;
}

function applyLayer<T>(entry: LayerEntry<T>, value: T): T {
  try {
    return entry.apply(value);
  } catch (error) {
    if (process.env.NODE_ENV !== "production") {
      console.warn("[rezejs] An optimistic layer threw and was skipped.", error);
    }
    return value;
  }
}

/**
 * A derived view of `source()` with temporary layers over it, for optimistic updates. Layers run
 * lazily on read, tracked, in insertion order; a layer that throws is skipped.
 */
export function optimistic<T>(source: () => T, options?: ComputedOptions): [Getter<T>, Layer<T>] {
  const layers: LayerEntry<T>[] = [];
  const changed = new SignalNode<undefined>(undefined, false);
  const shown = computed(() => {
    changed.read();
    let value = source();
    for (let i = 0; i < layers.length; i++) {
      value = applyLayer(layers[i]!, value);
    }
    return value;
  }, options);
  const layer: Layer<T> = (apply, until) => {
    const entry: LayerEntry<T> = { apply };
    layers.push(entry);
    changed.write(undefined);
    const remove = (): void => {
      const index = layers.indexOf(entry);
      if (index >= 0) {
        layers.splice(index, 1);
        changed.write(undefined);
      }
    };
    onCleanup(remove);
    if (until !== undefined) {
      Promise.resolve(until).then(remove, remove);
    }
    return remove;
  };
  return [shown, layer];
}
