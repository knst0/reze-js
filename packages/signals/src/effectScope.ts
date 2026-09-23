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

export function effectScope(fn: () => void): () => void {
  const e = new EffectScopeNode();
  const prevSub = enterOwner(e);
  try {
    fn();
  } finally {
    setActiveSub(prevSub);
  }
  return effectScopeOper.bind(e);
}

export function isEffectScope(fn: () => void): boolean {
  return fn.name === "bound " + effectScopeOper.name;
}

function effectScopeOper(this: EffectScopeNode): void {
  this.dispose();
}
