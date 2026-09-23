// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { effectDepth, setActiveSub } from "./context";
import { FlagNone, FlagRecursedCheck, FlagWatching } from "./flags";
import { propagate, type ReactiveNode, shallowPropagate, unlink } from "./graph";
import { endBatch, startBatch } from "./scheduler";

/** Runs `fn`, then notifies subscribers of every dependency it read. */
export function trigger(fn: () => void): void {
  const sub: ReactiveNode = {
    deps: undefined,
    depsTail: undefined,
    flags: FlagWatching | FlagRecursedCheck,
  };
  const prevSub = setActiveSub(sub);
  startBatch();
  try {
    fn();
  } finally {
    setActiveSub(prevSub);
    sub.flags = FlagNone;
    let link = sub.deps;
    while (link !== undefined) {
      const dep = link.dep;
      link = unlink(link, sub);
      const subs = dep.subs;
      if (subs !== undefined) {
        propagate(subs, effectDepth !== 0);
        shallowPropagate(subs);
      }
    }
    endBatch();
  }
}
