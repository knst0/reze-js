import { expect, test } from "vite-plus/test";

import { computed, effect, effectScope, flush, signal } from "../src";

test("a diamond settles each branch once per flush and never mixes old and new values", () => {
  const [source, setSource] = signal(1);
  const left = computed(() => source() * 2);
  const right = computed(() => source() + 10);
  const joinedPairs: [number, number][] = [];
  let joinedRuns = 0;
  const joined = computed(() => {
    joinedRuns++;
    const pair: [number, number] = [left(), right()];
    joinedPairs.push(pair);
    return pair[0] + pair[1];
  });
  const seen: number[] = [];
  let effectRuns = 0;
  effect(() => {
    effectRuns++;
    seen.push(joined());
  });

  expect(seen).toEqual([13]);
  expect(joinedRuns).toBe(1);

  setSource(2);
  flush();
  expect(seen).toEqual([13, 16]);
  expect(joinedRuns).toBe(2);
  expect(effectRuns).toBe(2);

  setSource(3);
  flush();
  expect(seen).toEqual([13, 16, 19]);
  expect(joinedRuns).toBe(3);
  expect(effectRuns).toBe(3);
  expect(joinedPairs).toEqual([
    [2, 11],
    [4, 12],
    [6, 13],
  ]);
});

test("two writes in one task produce one effect run after the microtask", async () => {
  const [a, setA] = signal(0);
  const [b, setB] = signal(0);
  const seen: [number, number][] = [];
  effect(() => {
    seen.push([a(), b()]);
  });

  setA(1);
  setB(2);
  expect(seen).toEqual([[0, 0]]);
  expect(a()).toBe(1);
  expect(b()).toBe(2);

  await Promise.resolve();
  expect(seen).toEqual([
    [0, 0],
    [1, 2],
  ]);
});

test("flush in the middle of a task splits coalesced writes into two runs", () => {
  const [value, setValue] = signal(0);
  const seen: number[] = [];
  effect(() => {
    seen.push(value());
  });

  setValue(1);
  flush();
  expect(seen).toEqual([0, 1]);

  setValue(2);
  expect(seen).toEqual([0, 1]);

  flush();
  expect(seen).toEqual([0, 1, 2]);
});

test("an effect writing another signal during a flush runs its downstream effect in the same flush", () => {
  const [source, setSource] = signal(0);
  const [mirrored, setMirrored] = signal(0);
  let upstreamRuns = 0;
  effect(() => {
    upstreamRuns++;
    setMirrored(source() * 10);
  });
  const seen: number[] = [];
  effect(() => {
    seen.push(mirrored());
  });
  expect(seen).toEqual([0]);

  setSource(1);
  flush();

  expect(upstreamRuns).toBe(2);
  expect(seen).toEqual([0, 10]);
});

test("an effect disposed by an earlier effect in the same flush never runs", () => {
  const [source, setSource] = signal(0);
  const log: string[] = [];
  let victimRuns = 0;
  let disposeVictim: () => void = () => {};
  effect(() => {
    if (source() === 1) {
      disposeVictim();
    }
  });
  disposeVictim = effect(() => {
    source();
    victimRuns++;
    effect(() => () => log.push("inner1:cleanup"));
    effect(() => () => log.push("inner2:cleanup"));
    return () => log.push("victim:cleanup");
  });
  effect(() => {
    if (source() === 1) {
      log.push("later:run");
    }
  });
  expect(victimRuns).toBe(1);

  setSource(1);
  flush();

  expect(victimRuns).toBe(1);
  expect(log).toEqual(["inner2:cleanup", "inner1:cleanup", "victim:cleanup", "later:run"]);

  disposeVictim();
  disposeVictim();
  expect(log).toEqual(["inner2:cleanup", "inner1:cleanup", "victim:cleanup", "later:run"]);
});

test("disposing an inner scope leaves the outer scope and sibling scopes alive", () => {
  const [source, setSource] = signal(0);
  let outerRuns = 0;
  let firstScopeRuns = 0;
  let secondScopeRuns = 0;
  let disposeFirstScope: () => void = () => {};
  const disposeOuterScope = effectScope(() => {
    effect(() => {
      source();
      outerRuns++;
    });
    disposeFirstScope = effectScope(() => {
      effect(() => {
        source();
        firstScopeRuns++;
      });
    });
    effectScope(() => {
      effect(() => {
        source();
        secondScopeRuns++;
      });
    });
  });
  expect([outerRuns, firstScopeRuns, secondScopeRuns]).toEqual([1, 1, 1]);

  disposeFirstScope();
  setSource(1);
  flush();
  expect([outerRuns, firstScopeRuns, secondScopeRuns]).toEqual([2, 1, 2]);

  disposeOuterScope();
  setSource(2);
  flush();
  expect([outerRuns, firstScopeRuns, secondScopeRuns]).toEqual([2, 1, 2]);
});

test("disposing an outer scope cascades to its contents newest first", () => {
  const log: string[] = [];
  const disposeOuterScope = effectScope(() => {
    effect(() => () => log.push("first:cleanup"));
    effectScope(() => {
      effect(() => () => log.push("inner:cleanup"));
    });
    effect(() => () => log.push("last:cleanup"));
  });

  disposeOuterScope();
  expect(log).toEqual(["last:cleanup", "inner:cleanup", "first:cleanup"]);

  disposeOuterScope();
  expect(log).toEqual(["last:cleanup", "inner:cleanup", "first:cleanup"]);
});

test("disposing a scope with a queued unflushed effect prevents the run", () => {
  const [source, setSource] = signal(0);
  let runs = 0;
  let cleanups = 0;
  const disposeScope = effectScope(() => {
    effect(() => {
      source();
      runs++;
      return () => {
        cleanups++;
      };
    });
  });
  expect(runs).toBe(1);

  setSource(1);
  disposeScope();
  flush();

  expect(runs).toBe(1);
  expect(cleanups).toBe(1);
});
