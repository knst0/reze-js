import { afterEach, expect, test } from "vite-plus/test";

import { FlagRecursed, FlagWatching } from "../src/flags";
import type { ReactiveNode } from "../src/graph";
import { flush, scheduleNode } from "../src/scheduler";

afterEach(flush);

test("watching owners execute outermost first", () => {
  const runs: string[] = [];
  const outer: ReactiveNode = {
    flags: FlagWatching,
    run: () => {
      runs.push("outer");
    },
  };
  const inner: ReactiveNode = {
    flags: FlagWatching,
    run: () => {
      runs.push("inner");
    },
  };
  const leaf: ReactiveNode = {
    flags: FlagWatching,
    run: () => {
      runs.push("leaf");
    },
  };
  const ownership: [ReactiveNode, ReactiveNode][] = [
    [leaf, inner],
    [inner, outer],
  ];
  for (const [dep, sub] of ownership) {
    dep.subs = { version: 0, dep, sub, prevSub: undefined, nextSub: undefined, prevDep: undefined, nextDep: undefined };
  }
  scheduleNode(leaf);
  flush();
  expect(runs).toEqual(["outer", "inner", "leaf"]);
});

test("nested flush preserves pending and newly queued node order", () => {
  const runs: string[] = [];
  const last: ReactiveNode = {
    flags: FlagWatching,
    run: () => {
      runs.push("last");
    },
  };
  const first: ReactiveNode = {
    flags: FlagWatching,
    run() {
      runs.push("first");
      scheduleNode(last);
      flush();
      runs.push("first resumed");
    },
  };
  const middle: ReactiveNode = {
    flags: FlagWatching,
    run: () => {
      runs.push("middle");
    },
  };
  scheduleNode(first);
  scheduleNode(middle);
  flush();
  expect(runs).toEqual(["first", "middle", "last", "first resumed"]);
});

test("throwing flush restores pending nodes so they can be queued again", () => {
  const error = new Error("scheduler failure");
  let runs = 0;
  const pending: ReactiveNode = {
    flags: FlagWatching,
    run: () => {
      runs++;
    },
  };
  const throwing: ReactiveNode = {
    flags: FlagWatching,
    run() {
      scheduleNode(pending);
      throw error;
    },
  };
  scheduleNode(throwing);
  expect(flush).toThrow(error);
  expect(pending.flags & (FlagWatching | FlagRecursed)).toBe(FlagWatching | FlagRecursed);
  flush();
  expect(runs).toBe(0);
  scheduleNode(pending);
  flush();
  expect(runs).toBe(1);
});

test("a drained nested flush may append another node before returning", () => {
  const runs: string[] = [];
  const last: ReactiveNode = {
    flags: FlagWatching,
    run: () => {
      runs.push("last");
    },
  };
  const middle: ReactiveNode = {
    flags: FlagWatching,
    run: () => {
      runs.push("middle");
    },
  };
  const first: ReactiveNode = {
    flags: FlagWatching,
    run() {
      scheduleNode(middle);
      flush();
      scheduleNode(last);
    },
  };
  scheduleNode(first);
  flush();
  expect(runs).toEqual(["middle", "last"]);
});

test("flush executes an entire cascading chain in the same call", () => {
  let runs = 0;
  const next = (): ReactiveNode => ({
    flags: FlagWatching,
    run() {
      if (++runs < 1000) scheduleNode(next());
    },
  });
  scheduleNode(next());
  flush();
  expect(runs).toBe(1000);
  flush();
  expect(runs).toBe(1000);
});
