import { expect, test } from "vitest";

import { FlagDirty, FlagPending, FlagRecursedCheck, FlagWatching } from "../src/flags";
import { link, shallowPropagate, type ReactiveNode } from "../src/graph";
import { flush } from "../src/scheduler";

test("shallow propagation preserves unrelated bits for every node flag combination", () => {
  for (let flags = 0; flags < 128; flags++) {
    let runs = 0;
    const dep: ReactiveNode = { flags: 0 };
    const sub: ReactiveNode = { flags, run: () => runs++ };
    link(dep, sub, 1);
    const subscriberLink = dep.subs!;
    const becomesDirty = (flags & (FlagPending | FlagDirty)) === FlagPending;
    const queued = becomesDirty && (flags & (FlagWatching | FlagRecursedCheck)) === FlagWatching;

    shallowPropagate(subscriberLink);

    expect(sub.flags, `flags=${flags}`).toBe(queued ? (flags | FlagDirty) & ~FlagWatching : becomesDirty ? flags | FlagDirty : flags);
    expect(dep.subs).toBe(subscriberLink);
    expect(dep.subsTail).toBe(subscriberLink);
    expect(sub.deps).toBe(subscriberLink);
    expect(sub.depsTail).toBe(subscriberLink);
    expect(runs).toBe(0);
    flush();
    expect(runs, `flags=${flags}`).toBe(queued ? 1 : 0);
  }
});

test("shallow propagation queues owners first and follows only the requested subscriber suffix", () => {
  const order: string[] = [];
  const dep: ReactiveNode = { flags: 0 };
  const skipped: ReactiveNode = { flags: FlagPending | FlagWatching, run: () => order.push("skipped") };
  const first: ReactiveNode = { flags: FlagPending | FlagWatching, run: () => order.push("first") };
  const second: ReactiveNode = { flags: FlagPending | FlagWatching, run: () => order.push("second") };
  const owner: ReactiveNode = { flags: FlagWatching, run: () => order.push("owner") };
  const descendant: ReactiveNode = { flags: FlagPending | FlagWatching, run: () => order.push("descendant") };
  link(dep, skipped, 1);
  link(dep, first, 1);
  link(dep, second, 1);
  link(first, owner, 1);
  link(second, descendant, 1);
  descendant.flags &= ~FlagWatching;
  const start = dep.subs!.nextSub!;

  shallowPropagate(start);
  shallowPropagate(start);

  expect(order).toEqual([]);
  expect(skipped.flags).toBe(FlagPending | FlagWatching);
  expect(first.flags).toBe(FlagPending | FlagDirty);
  expect(second.flags).toBe(FlagPending | FlagDirty);
  expect(descendant.flags).toBe(FlagPending);
  expect(start.nextSub).toBe(dep.subsTail);
  flush();
  expect(order).toEqual(["owner", "first", "second"]);
});
