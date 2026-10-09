import { expect, test } from "vite-plus/test";

import { link, unlink, type ReactiveNode } from "../src/graph";

const node = (): ReactiveNode => ({ flags: 0 });

for (const depPosition of [0, 1, 2]) {
  for (const subPosition of [0, 1, 2]) {
    test(`T12 unlink preserves both intrusive lists at positions ${depPosition}/${subPosition}`, () => {
      const deps = [node(), node(), node()];
      const subs = [node(), node(), node()];
      const dep = deps[depPosition]!;
      const sub = subs[subPosition]!;
      let calls = 0;
      dep.unwatched = () => {
        calls++;
      };
      for (let index = 0; index < subPosition; index++) link(dep, subs[index]!, 1);
      for (const current of deps) link(current, sub, 1);
      let target = sub.deps!;
      for (let index = 0; index < depPosition; index++) target = target.nextDep!;
      for (let index = subPosition + 1; index < subs.length; index++) link(dep, subs[index]!, 1);
      const nextDep = target.nextDep;
      const prevDep = target.prevDep;
      const nextSub = target.nextSub;
      const prevSub = target.prevSub;
      expect(unlink(target)).toBe(nextDep);
      expect(calls).toBe(0);
      expect(prevDep !== undefined ? prevDep.nextDep : sub.deps).toBe(nextDep);
      expect(nextDep !== undefined ? nextDep.prevDep : sub.depsTail).toBe(prevDep);
      expect(prevSub !== undefined ? prevSub.nextSub : dep.subs).toBe(nextSub);
      expect(nextSub !== undefined ? nextSub.prevSub : dep.subsTail).toBe(prevSub);
    });
  }
}

test("T12 last-subscriber callback sees detached lists and can resubscribe", () => {
  const dep = node();
  const next = node();
  const sub = node();
  link(dep, sub, 1);
  link(next, sub, 1);
  const target = sub.deps!;
  const savedNext = target.nextDep;
  const replacement = node();
  let calls = 0;
  dep.unwatched = () => {
    calls++;
    expect(dep.subs).toBeUndefined();
    expect(dep.subsTail).toBeUndefined();
    expect(sub.deps).toBe(savedNext);
    expect(savedNext?.prevDep).toBeUndefined();
    unlink(savedNext!);
    link(dep, replacement, 2);
  };
  expect(unlink(target, sub)).toBe(savedNext);
  expect(calls).toBe(1);
  expect(sub.deps).toBeUndefined();
  expect(sub.depsTail).toBeUndefined();
  expect(dep.subs?.sub).toBe(replacement);
  expect(dep.subsTail).toBe(dep.subs);
});

test("T12 callback exceptions propagate after complete detachment", () => {
  const dep = node();
  const sub = node();
  const error = new Error("unwatched failed");
  dep.unwatched = () => {
    throw error;
  };
  link(dep, sub, 1);
  expect(() => unlink(sub.deps!)).toThrow(error);
  expect(sub.deps).toBeUndefined();
  expect(sub.depsTail).toBeUndefined();
  expect(dep.subs).toBeUndefined();
  expect(dep.subsTail).toBeUndefined();
});
