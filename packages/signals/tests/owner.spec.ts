import { expect, test } from "vitest";

import {
  batch,
  effect,
  flushSync,
  getOwner,
  onCleanup,
  root,
  runWithOwner,
  signal,
  untrack,
  type Owner,
} from "../src";

test("root dispose runs cleanups and effect teardowns in reverse creation order", () => {
  const log: string[] = [];
  const dispose = root((dispose) => {
    onCleanup(() => log.push("c1"));
    effect(() => () => log.push("e"));
    onCleanup(() => log.push("c2"));
    return dispose;
  });
  expect(log).toEqual([]);
  dispose();
  expect(log).toEqual(["c2", "e", "c1"]);
});

test("untrack hides reads from the effect but keeps what it creates owned by it", () => {
  const [a, setA] = signal(0);
  const [b, setB] = signal(0);
  const log: string[] = [];
  root(() => {
    effect(() => {
      a();
      untrack(() => {
        b();
        onCleanup(() => log.push("inner cleanup"));
      });
    });
  });

  setB(1);
  flushSync();
  expect(log).toEqual([]);
  setA(1);
  flushSync();
  expect(log).toEqual(["inner cleanup"]);
});

test("runWithOwner attaches work created later to that owner", () => {
  const log: string[] = [];
  let owner: Owner | undefined;
  const dispose = root((dispose) => {
    owner = getOwner();
    return dispose;
  });
  runWithOwner(owner, () => onCleanup(() => log.push("late")));
  dispose();
  expect(log).toEqual(["late"]);
});

test("batch defers effects until the outermost batch exits", () => {
  const [a, setA] = signal(0);
  const seen: number[] = [];
  effect(() => {
    seen.push(a());
  });
  batch(() => {
    setA(1);
    batch(() => setA(2));
    expect(seen).toEqual([0]);
  });
  expect(seen).toEqual([0, 2]);
});
