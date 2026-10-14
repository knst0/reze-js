import { expect, test } from "vite-plus/test";

import { asyncComputed, effect, flush, root, signal, type AsyncComputed } from "../src";

function tick(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

async function tracksSourcesOfTheLatestRun(): Promise<void> {
  const [id, setId] = signal(1);
  const [lang, setLang] = signal("en");
  const runs: string[] = [];
  const text = asyncComputed(async (c) => {
    const current = c.get(id);
    await tick();
    const label = `${current}:${c.get(lang)}`;
    runs.push(label);
    return label;
  });
  await tick();
  await tick();
  expect(text.value()).toBe("1:en");

  setLang("fr");
  flush();
  await tick();
  await tick();
  expect(text.value()).toBe("1:fr");

  setId(2);
  flush();
  await tick();
  await tick();
  expect(runs).toEqual(["1:en", "1:fr", "2:fr"]);
}

async function supersededReadsAreUntracked(): Promise<void> {
  const [id, setId] = signal(1);
  const [late, setLate] = signal(0);
  const gate = Promise.withResolvers<void>();
  let runs = 0;
  asyncComputed(async (c) => {
    runs++;
    const current = c.get(id);
    if (current === 1) {
      await gate.promise;
      c.get(late);
    }
    return current;
  });
  setId(2);
  flush();
  gate.resolve();
  await tick();

  setLate(1);
  flush();
  await tick();
  expect(runs).toBe(2);
}

async function droppedSourceStopsRerunning(): Promise<void> {
  const [useA, setUseA] = signal(true);
  const [a, setA] = signal("a");
  let runs = 0;
  asyncComputed(async (c) => {
    runs++;
    return c.get(useA) ? c.get(a) : "none";
  });
  await tick();
  setUseA(false);
  flush();
  await tick();
  expect(runs).toBe(2);

  setA("a2");
  flush();
  await tick();
  expect(runs).toBe(2);
}

test("a run tracks the sources it reads, including after an await, and drops the ones it stops reading", async () => {
  await tracksSourcesOfTheLatestRun();
  await supersededReadsAreUntracked();
  await droppedSourceStopsRerunning();
});

test("a superseded run never overwrites the latest one", async () => {
  const [id, setId] = signal(1);
  const requests = new Map<number, PromiseWithResolvers<string>>();
  const user = asyncComputed((c) => {
    const request = Promise.withResolvers<string>();
    requests.set(c.get(id), request);
    return request.promise;
  });
  setId(2);
  flush();

  requests.get(2)!.resolve("second");
  await tick();
  requests.get(1)!.resolve("first");
  await tick();
  expect(user.value()).toBe("second");
  expect(user.isPending()).toBe(false);
});

test("a rejection or synchronous throw is exposed as error(), keeps the last value, and clears on the next success", async () => {
  const [id, setId] = signal(1);
  const user = asyncComputed(async (c) => {
    const current = c.get(id);
    if (current === 2) throw new Error("boom");
    return current;
  });
  await tick();
  setId(2);
  flush();
  await tick();
  expect((user.error() as Error).message).toBe("boom");
  expect(user.value()).toBe(1);
  expect(user.isPending()).toBe(false);

  setId(3);
  flush();
  await tick();
  expect(user.error()).toBeUndefined();
  expect(user.value()).toBe(3);

  const sync = asyncComputed((): Promise<number> => {
    throw new Error("sync");
  });
  await tick();
  expect((sync.error() as Error).message).toBe("sync");
});

test("readers re-run when the state they read changes", async () => {
  const [id, setId] = signal(1);
  const user = asyncComputed(async (c) => c.get(id) * 10);
  const seen: unknown[] = [];
  effect(() => {
    seen.push([user.isPending(), user.value()]);
  });
  await tick();
  setId(2);
  flush();
  await tick();
  expect(seen).toEqual([
    [true, undefined],
    [false, 10],
    [true, 10],
    [false, 20],
  ]);
});

test("disposing the owner drops a pending settlement and stops re-running", async () => {
  const [id, setId] = signal(1);
  const request = Promise.withResolvers<string>();
  let runs = 0;
  let user!: AsyncComputed<string>;
  const dispose = root((dispose) => {
    user = asyncComputed((c) => {
      runs++;
      c.get(id);
      return request.promise;
    });
    return dispose;
  });
  dispose();
  request.resolve("late");
  await tick();
  expect(user.value()).toBeUndefined();

  setId(2);
  flush();
  expect(runs).toBe(1);
});
