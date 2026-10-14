import { expect, test } from "vite-plus/test";

import { effect, flush, getOwner, onCleanup, root, runWithOwner, signal, untrack, type Owner } from "../src";

test("root dispose runs cleanups and effect teardowns in reverse creation order, and runWithOwner attaches later work to the owner", () => {
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

  const later: string[] = [];
  let owner: Owner | undefined;
  const disposeLater = root((dispose) => {
    owner = getOwner();
    return dispose;
  });
  runWithOwner(owner, () => onCleanup(() => later.push("late")));
  disposeLater();
  expect(later).toEqual(["late"]);
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
  flush();
  expect(log).toEqual([]);
  setA(1);
  flush();
  expect(log).toEqual(["inner cleanup"]);
});

test("writes stay async until flush drains them", () => {
  const [a, setA] = signal(0);
  const seen: number[] = [];
  effect(() => {
    seen.push(a());
  });
  setA(1);
  setA(2);
  expect(seen).toEqual([0]);
  flush();
  expect(seen).toEqual([0, 2]);
});
