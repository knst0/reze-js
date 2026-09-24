import { effectDepth, getActiveSub, track } from "./context";
import { FlagDirty, FlagMutable } from "./flags";
import { propagate, shallowPropagate, type Link, type ReactiveNode } from "./graph";
import { renderEffect } from "./render";
import { scheduleFlush } from "./scheduler";
import type { Getter } from "./signal";

class SelectorKeyNode<K> implements ReactiveNode {
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  flags: number = FlagMutable;
  currentValue: boolean;
  pendingValue: boolean;
  key: K;
  owners: Map<K, SelectorKeyNode<K>>;

  constructor(key: K, isSelected: boolean, owners: Map<K, SelectorKeyNode<K>>) {
    this.key = key;
    this.currentValue = isSelected;
    this.pendingValue = isSelected;
    this.owners = owners;
  }

  update(): boolean {
    this.flags = FlagMutable;
    const changed = this.currentValue !== this.pendingValue;
    this.currentValue = this.pendingValue;
    return changed;
  }

  write(isSelected: boolean): void {
    if (this.pendingValue === isSelected) {
      return;
    }
    this.pendingValue = isSelected;
    this.flags = FlagMutable | FlagDirty;
    const subs = this.subs;
    if (subs !== undefined) {
      propagate(subs, effectDepth !== 0);
      scheduleFlush();
    }
  }

  read(): boolean {
    if (this.flags & FlagDirty && this.update()) {
      const subs = this.subs;
      if (subs !== undefined) {
        shallowPropagate(subs);
      }
    }
    track(this);
    return this.currentValue;
  }

  unwatched(): void {
    if (this.owners.get(this.key) === this) {
      this.owners.delete(this.key);
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
export function selector<K>(
  source: Getter<K>,
  equals?: (key: K, value: K) => boolean,
): (key: K) => boolean {
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
    nodes.get(previous)?.write(false);
    nodes.get(value)?.write(true);
  });

  return (key) => {
    if (getActiveSub() === undefined) {
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
