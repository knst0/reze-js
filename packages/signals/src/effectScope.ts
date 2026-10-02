// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { enterOwner, setActiveSub } from "./context";
import { FlagMutable } from "./flags";
import { disposeNode, type Link, type ReactiveNode } from "./graph";

class EffectScopeNode implements ReactiveNode {
  deps: Link | undefined = undefined;
  depsTail: Link | undefined = undefined;
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  flags: number = FlagMutable;

  update(): boolean {
    this.flags = FlagMutable;
    return true;
  }

  unwatched(): void {
    this.dispose();
  }

  dispose(): void {
    disposeNode(this);
  }
}

/**
 * Runs `fn` in a new owner that lives as long as the current one. Returns the disposer; reads in
 * `fn` are tracked by the scope and do not re-run it.
 */
export function effectScope(fn: () => void): () => void {
  const node = new EffectScopeNode();
  const prevSub = enterOwner(node);
  try {
    fn();
  } finally {
    setActiveSub(prevSub);
  }
  return (): void => {
    node.dispose();
  };
}
