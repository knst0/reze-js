import { expect, test } from "vite-plus/test";

import { link, purgeDeps, type ReactiveNode } from "../src/graph";

const node = (): ReactiveNode => ({ flags: 0 });

test("T11 consecutive reads retain the same intrusive link", () => {
  const dep = node();
  const sub = node();
  link(dep, sub, 1);
  const first = sub.deps;
  for (let i = 0; i < 1000; i++) link(dep, sub, 1);
  expect(sub.deps).toBe(first);
  expect(sub.depsTail).toBe(first);
  expect(dep.subsTail).toBe(first);
  expect(first?.nextDep).toBeUndefined();
  expect(first?.nextSub).toBeUndefined();
});

test("T11 ordered reruns reuse links and refresh versions", () => {
  const a = node();
  const b = node();
  const sub = node();
  link(a, sub, 1);
  link(b, sub, 1);
  const first = sub.deps;
  const second = sub.depsTail;
  for (let version = 2; version <= 1001; version++) {
    sub.depsTail = undefined;
    link(a, sub, version);
    link(b, sub, version);
    purgeDeps(sub);
  }
  expect(sub.deps).toBe(first);
  expect(sub.depsTail).toBe(second);
  expect(first?.version).toBe(1001);
  expect(second?.version).toBe(1001);
  expect(first?.nextDep).toBe(second);
  expect(second?.prevDep).toBe(first);
});

test("T11 nonconsecutive duplicate reads do not advance the dependency tail", () => {
  const a = node();
  const b = node();
  const sub = node();
  link(a, sub, 1);
  link(b, sub, 1);
  const tail = sub.depsTail;
  const first = a.subs;
  link(a, sub, 1);
  expect(sub.depsTail).toBe(tail);
  expect(a.subsTail).toBe(first);
  expect(tail?.nextDep).toBeUndefined();
});

test("T11 reordered reads insert then purge stale links with both lists intact", () => {
  const a = node();
  const b = node();
  const sub = node();
  link(a, sub, 1);
  link(b, sub, 1);
  const oldA = a.subs;
  const oldB = b.subs;
  sub.depsTail = undefined;
  link(b, sub, 2);
  const newB = b.subsTail;
  link(a, sub, 2);
  purgeDeps(sub);
  expect(newB).not.toBe(oldB);
  expect(sub.deps).toBe(newB);
  expect(sub.depsTail).toBe(oldA);
  expect(newB?.nextDep).toBe(oldA);
  expect(oldA?.prevDep).toBe(newB);
  expect(oldA?.nextDep).toBeUndefined();
  expect(b.subs).toBe(newB);
  expect(b.subsTail).toBe(newB);
  expect(newB?.prevSub).toBeUndefined();
  expect(newB?.nextSub).toBeUndefined();
});
