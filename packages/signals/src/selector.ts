import { activeSub } from "./context";
import { renderEffect } from "./render";
import { type Getter, SignalNode } from "./signal";

class SelectorKeyNode<K> extends SignalNode<boolean> {
  key: K;
  nodes: Map<K, SelectorKeyNode<K>>;

  constructor(key: K, isSelected: boolean, nodes: Map<K, SelectorKeyNode<K>>) {
    super(isSelected, Object.is);
    this.key = key;
    this.nodes = nodes;
  }

  unwatched(): void {
    if (this.nodes.get(this.key) === this) {
      this.nodes.delete(this.key);
    }
  }
}

/**
 * Returns `isSelected(key)`, which is `equals(key, source())` (default `Object.is`) and
 * subscribes its caller to that key's result only: when `source` changes, only the callers
 * whose result flipped re-run, so selecting one row out of n costs O(1) instead of O(n).
 * With a custom `equals` every live key is re-checked on each change, which is O(live keys).
 * Lives as long as the current owner.
 */
export function selector<K>(source: Getter<K>, equals?: (key: K, value: K) => boolean): (key: K) => boolean {
  const nodes = new Map<K, SelectorKeyNode<K>>();
  const matches = equals ?? Object.is;
  let value!: K;
  let isInitialized = false;

  renderEffect(() => {
    const previous = value;
    value = source();
    if (!isInitialized) {
      isInitialized = true;
      return;
    }
    if (Object.is(previous, value)) {
      return;
    }
    if (equals !== undefined) {
      for (const node of nodes.values()) node.write(equals(node.key, value));
      return;
    }
    const deselected = nodes.get(previous);
    if (deselected !== undefined) deselected.write(false);
    const selected = nodes.get(value);
    if (selected !== undefined) selected.write(true);
  });

  return (key) => {
    if (activeSub === undefined) {
      return matches(key, value);
    }
    let node = nodes.get(key);
    if (node === undefined) {
      node = new SelectorKeyNode(key, matches(key, value), nodes);
      nodes.set(key, node);
    }
    return node.read();
  };
}
