import { expect, test } from "vite-plus/test";

import { FlagMutable, FlagPending, FlagRecursed, FlagWatching } from "../src/flags";
import { link, propagate, type ReactiveNode } from "../src/graph";
import { flush } from "../src/scheduler";

function mutable(): ReactiveNode {
  return { flags: FlagMutable };
}

function watcher(name: string, order: string[], run?: () => void): ReactiveNode {
  return {
    flags: FlagWatching,
    run() {
      order.push(name);
      run?.();
    },
  };
}

function countForkPushes(run: () => void): number {
  const original = Array.prototype.push;
  let count = 0;
  Array.prototype.push = function (...values) {
    count += values.length;
    return original.apply(this, values);
  };
  try {
    run();
  } finally {
    Array.prototype.push = original;
  }
  return count;
}

test("a straight mutable chain uses no fork pushes", () => {
  const root = mutable();
  const chain = Array.from({ length: 32 }, mutable);
  const order: string[] = [];
  const leaf = watcher("leaf", order);
  let previous = root;
  for (const node of [...chain, leaf]) {
    link(previous, node, 1);
    previous = node;
  }

  expect(countForkPushes(() => propagate(root.subs!, false))).toBe(0);
  expect(chain.every((node) => node.flags === (FlagMutable | FlagPending))).toBe(true);
  flush();
  expect(order).toEqual(["leaf"]);
});

test("a sole mutable subscriber uses one local fork push for its fanout", () => {
  const root = mutable();
  const branch = mutable();
  const order: string[] = [];
  link(root, branch, 1);
  for (const name of ["first", "second", "third"]) {
    link(branch, watcher(name, order), 1);
  }

  expect(countForkPushes(() => propagate(root.subs!, false))).toBe(1);
  flush();
  expect(order).toEqual(["first", "second", "third"]);
});

test("nested fanouts retain live continuations in depth-first order", () => {
  const root = mutable();
  const branch = mutable();
  const nested = mutable();
  const order: string[] = [];
  link(root, branch, 1);
  link(root, watcher("root-tail", order), 1);
  link(branch, nested, 1);
  link(branch, watcher("branch-tail", order), 1);
  link(nested, watcher("nested-first", order), 1);
  link(nested, watcher("nested-second", order), 1);

  expect(countForkPushes(() => propagate(root.subs!, false))).toBe(2);
  flush();
  expect(order).toEqual(["nested-first", "nested-second", "branch-tail", "root-tail"]);
});

test("a diamond marks shared descendants once and retains inner-write flags", () => {
  const root = mutable();
  const left = mutable();
  const right = mutable();
  const shared = mutable();
  const order: string[] = [];
  const leaf = watcher("shared", order);
  link(root, left, 1);
  link(root, right, 1);
  link(left, shared, 1);
  link(right, shared, 1);
  link(shared, leaf, 1);

  propagate(root.subs!, true);
  expect(left.flags).toBe(FlagMutable | FlagPending | FlagRecursed);
  expect(right.flags).toBe(FlagMutable | FlagPending | FlagRecursed);
  expect(shared.flags).toBe(FlagMutable | FlagPending);
  flush();
  expect(order).toEqual(["shared"]);
});

test("a watcher may recursively propagate a separate fanout while flushing", () => {
  const root = mutable();
  const branch = mutable();
  const innerRoot = mutable();
  const innerBranch = mutable();
  const order: string[] = [];
  link(root, branch, 1);
  link(root, watcher("outer-tail", order), 1);
  link(
    branch,
    watcher("outer-first", order, () => propagate(innerRoot.subs!, true)),
    1,
  );
  link(branch, watcher("outer-second", order), 1);
  link(innerRoot, innerBranch, 1);
  link(innerBranch, watcher("inner-first", order), 1);
  link(innerBranch, watcher("inner-second", order), 1);

  propagate(root.subs!, false);
  flush();
  expect(order).toEqual(["outer-first", "outer-second", "outer-tail", "inner-first", "inner-second"]);
});
