import { expect, test, vi } from "vite-plus/test";

import { cache, revalidateTag } from "../src/cache";

test("a cached loader shares one promise per argument tuple and revalidates by tag", async () => {
  let calls = 0;
  const load = cache(
    async (id: string) => {
      calls += 1;
      return `${id}:${calls}`;
    },
    { ttl: 60_000, tags: (id: string) => [`user:${id}`] },
  );
  expect(await load("a")).toBe("a:1");
  expect(await load("a")).toBe("a:1");
  expect(calls).toBe(1);
  expect(await load("b")).toBe("b:2");
  expect(revalidateTag("user:a")).toBe(1);
  expect(await load("a")).toBe("a:3");
  expect(revalidateTag("missing")).toBe(0);
  expect(await load("b")).toBe("b:2");
});

test("a rejected load is dropped so the next call retries", async () => {
  let calls = 0;
  const load = cache(
    async () => {
      calls += 1;
      if (calls === 1) throw new Error("first");
      return calls;
    },
    { ttl: 60_000 },
  );
  await expect(load()).rejects.toThrow("first");
  expect(await load()).toBe(2);
});

test("an unserializable argument calls through uncached", async () => {
  let calls = 0;
  const load = cache(
    async (value: () => number) => {
      calls += 1;
      return value() + calls;
    },
    { ttl: 60_000 },
  );
  expect(await load(() => 1)).toBe(2);
  expect(await load(() => 1)).toBe(3);
});

test("ttl expires an entry", async () => {
  vi.useFakeTimers();
  try {
    let calls = 0;
    const load = cache(
      async () => {
        calls += 1;
        return calls;
      },
      { ttl: 1000 },
    );
    expect(await load()).toBe(1);
    vi.advanceTimersByTime(1001);
    expect(await load()).toBe(2);
  } finally {
    vi.useRealTimers();
  }
});

test("invalid ttl is refused when the loader is created", () => {
  expect(() => cache(async () => 1, { ttl: -1 })).toThrow(/non-negative/);
  expect(() => cache(async () => 1, { ttl: Number.NaN })).toThrow(/non-negative/);
});
