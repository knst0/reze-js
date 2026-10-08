import { expect, test } from "vitest";

import { asyncComputed, flush, root, signal } from "../src";

function tick(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

test("the signal of a run aborts when a newer run starts and stays live for the latest", async () => {
  const [id, setId] = signal(1);
  const runSignals: AbortSignal[] = [];
  asyncComputed(async (c) => {
    const current = c.get(id);
    runSignals.push(c.abortSignal());
    await tick();
    return current;
  });
  await tick();
  await tick();
  expect(runSignals[0]!.aborted).toBe(false);

  setId(2);
  flush();
  await tick();
  await tick();
  expect(runSignals).toHaveLength(2);
  expect(runSignals[0]!.aborted).toBe(true);
  expect(runSignals[1]!.aborted).toBe(false);
});

test("disposing the computation aborts the signal of its latest run", async () => {
  let latest: AbortSignal | undefined;
  const dispose = root((dispose) => {
    asyncComputed(async (c) => {
      latest = c.abortSignal();
      await tick();
    });
    return dispose;
  });
  await tick();
  await tick();
  expect(latest!.aborted).toBe(false);

  dispose();
  expect(latest!.aborted).toBe(true);
});
