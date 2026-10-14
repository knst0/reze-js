import { expect, test } from "vite-plus/test";

import { computed, effect, flush, signal } from "../src";

test("a microtask flush left over after flush is a no-op (P03)", async () => {
  const [src, setSrc] = signal(0);
  let runs = 0;
  effect(() => {
    src();
    runs++;
  });
  flush();
  runs = 0;
  setSrc(1);
  flush();
  expect(runs).toBe(1);
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  await promise;
  expect(runs).toBe(1);
});

test("outer re-runs dispose inner effects first, including after a prior inner-only re-run", () => {
  const [a, setA] = signal(0);
  const [b, setB] = signal(0);
  const log: string[] = [];

  effect(() => {
    a();
    log.push("outer:run");
    effect(() => {
      b();
      log.push("inner:run");
      return () => log.push("inner:cleanup");
    });
    return () => log.push("outer:cleanup");
  });
  expect(log).toEqual(["outer:run", "inner:run"]);

  log.length = 0;
  setB(1);
  flush();
  log.length = 0;

  setA(1);
  flush();
  expect(log).toEqual(["inner:cleanup", "outer:cleanup", "outer:run", "inner:run"]);
});

test("effects created inside a computed are disposed LIFO on unwatch and before its next evaluation", () => {
  const log: string[] = [];
  const c = computed(() => {
    effect(() => () => log.push("e1"));
    effect(() => () => log.push("e2"));
    effect(() => () => log.push("e3"));
    return 0;
  });
  const dispose = effect(() => {
    c();
  });
  log.length = 0;
  dispose();
  expect(log).toEqual(["e3", "e2", "e1"]);

  const [a, setA] = signal(0);
  const trail: string[] = [];
  const derived = computed(() => {
    trail.push("computed:eval");
    effect(() => {
      trail.push("inner:run");
      return () => trail.push("inner:cleanup");
    });
    return a();
  });
  effect(() => {
    derived();
  });
  trail.length = 0;
  setA(1);
  flush();
  expect(trail).toEqual(["inner:cleanup", "computed:eval", "inner:run"]);
});

// https://github.com/stackblitz/alien-signals/issues/115
test("outer effect keeps responding to its own dep after inner re-runs", () => {
  const [a, setA] = signal(0);
  const [b, setB] = signal(0);
  let outerRuns = 0;
  let innerRuns = 0;

  effect(() => {
    a();
    outerRuns++;
    effect(() => {
      b();
      innerRuns++;
    });
  });
  expect(outerRuns).toBe(1);
  expect(innerRuns).toBe(1);

  setB(1);
  flush();
  expect(outerRuns).toBe(1);
  expect(innerRuns).toBeGreaterThanOrEqual(2);

  setA(1);
  flush();
  expect(outerRuns).toBe(2);
});
