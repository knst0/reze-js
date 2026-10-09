import { expect, test } from "vite-plus/test";

import { asyncComputed, effect, effectScope, flush, signal, withReloadScope } from "../src";

function tick(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

test("isPending is false for a first load and true while a re-run is pending", async () => {
  const [id, setId] = signal(1);
  const pending: boolean[] = [];
  withReloadScope((isPending) => {
    asyncComputed(async (c) => {
      const current = c.get(id);
      await tick();
      return current;
    });
    effect(() => {
      pending.push(isPending());
    });
  });
  await tick();
  await tick();

  setId(2);
  flush();
  await tick();
  await tick();
  expect(pending).toEqual([false, true, false]);
});

test("a reload below a nested scope is pending in every enclosing scope", async () => {
  const [id, setId] = signal(1);
  const outer: boolean[] = [];
  withReloadScope((isOuterPending) => {
    withReloadScope(() => {
      asyncComputed(async (c) => {
        const current = c.get(id);
        await tick();
        return current;
      });
    });
    effect(() => {
      outer.push(isOuterPending());
    });
  });
  await tick();
  await tick();

  setId(2);
  flush();
  await tick();
  await tick();
  expect(outer).toEqual([false, true, false]);
});

test("disposing a computation mid-reload releases its count", async () => {
  const [id, setId] = signal(1);
  const pending: boolean[] = [];
  withReloadScope((isPending) => {
    const disposeChild = effectScope(() => {
      asyncComputed(async (c) => {
        const current = c.get(id);
        await tick();
        return current;
      });
    });
    effect(() => {
      pending.push(isPending());
    });
    setId(2);
    flush();
    disposeChild();
  });
  await tick();
  await tick();
  expect(pending.at(-1)).toBe(false);
});
