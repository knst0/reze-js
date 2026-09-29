import { debugHook } from "./devtools";
import { type Getter, type Setter, SignalNode, type SignalOptions } from "./signal";

/**
 * Stacks `apply` over the base value, after every earlier layer, and returns a function removing
 * it. Removing a layer re-applies the ones above it to the base, and is a no-op the second time.
 * `apply` must be pure: it re-runs whenever the base or another layer changes. Throws when
 * `apply` or a later layer throws, leaving the signal unchanged.
 */
export type Layer<T> = (apply: (base: T) => T) => () => void;

interface LayerEntry<T> {
  apply: (base: T) => T;
}

class LayeredSignalNode<T> extends SignalNode<T> {
  base: T;
  layers: readonly LayerEntry<T>[] = [];

  constructor(value: T, options: SignalOptions<T> | undefined) {
    super(value, options?.equals ?? Object.is);
    this.base = value;
  }
}

function shown<T>(base: T, layers: readonly LayerEntry<T>[]): T {
  let value = base;
  for (let i = 0; i < layers.length; i++) {
    value = layers[i]!.apply(value);
  }
  return value;
}

function setBase<T>(this: LayeredSignalNode<T>, next: T | ((prev: T) => T)): T {
  const base = typeof next === "function" ? (next as (prev: T) => T)(this.base) : next;
  const value = shown(base, this.layers);
  this.base = base;
  this.write(value);
  return base;
}

function commitLayers<T>(node: LayeredSignalNode<T>, layers: readonly LayerEntry<T>[]): void {
  const value = shown(node.base, layers);
  node.layers = layers;
  node.write(value);
}

function addLayer<T>(this: LayeredSignalNode<T>, apply: (base: T) => T): () => void {
  const entry: LayerEntry<T> = { apply };
  commitLayers(this, [...this.layers, entry]);
  return () => {
    if (this.layers.includes(entry)) {
      commitLayers(
        this,
        this.layers.filter((layer) => layer !== entry),
      );
    }
  };
}

/**
 * Creates a signal whose shown value is a confirmed base with a stack of temporary layers over it,
 * for optimistic updates. The setter writes the base, and its updater form receives the base,
 * never the shown value. The getter reads the shown value, so a layer is visible at once and
 * disappears, rebased over the others, when removed.
 */
export function layeredSignal<T>(initialValue: T, options?: SignalOptions<T>): [Getter<T>, Setter<T>, Layer<T>] {
  const node = new LayeredSignalNode(initialValue, options);
  if (process.env.NODE_ENV !== "production" && debugHook !== undefined) {
    debugHook.created(node, "signal", options?.name, () => node.pendingValue);
  }
  return [node.read.bind(node), (setBase<T>).bind(node), (addLayer<T>).bind(node)];
}
