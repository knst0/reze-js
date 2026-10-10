import { expect, test, vi } from "vite-plus/test";

import { createQueryClient } from "../src";

test("fresh data is reused until invalidated, then fetched again", async () => {
  const client = createQueryClient();
  const queryFn = vi.fn(() => Promise.resolve("v1"));
  const options = { key: ["post", "1"] as const, queryFn, staleTime: 1_000 };

  expect(await client.fetchQuery(options)).toBe("v1");
  expect(await client.fetchQuery(options)).toBe("v1");
  expect(queryFn).toHaveBeenCalledTimes(1);

  client.invalidateQueries(["post"]);
  queryFn.mockResolvedValue("v2");
  expect(await client.fetchQuery(options)).toBe("v2");
  expect(queryFn).toHaveBeenCalledTimes(2);
});

test("concurrent callers share a request and failed work can be retried", async () => {
  const client = createQueryClient();
  const pending = Promise.withResolvers<string>();
  const queryFn = vi.fn(() => pending.promise);
  const options = { key: ["post", "2"] as const, queryFn };

  const first = client.fetchQuery(options);
  const second = client.fetchQuery(options);
  await Promise.resolve();
  expect(queryFn).toHaveBeenCalledTimes(1);

  pending.resolve("loaded");
  expect(await Promise.all([first, second])).toEqual(["loaded", "loaded"]);

  const retryFn = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce("online");
  const retryOptions = { key: ["retry"] as const, queryFn: retryFn, staleTime: 10_000 };
  await expect(client.fetchQuery(retryOptions)).rejects.toThrow("offline");
  expect(await client.fetchQuery(retryOptions)).toBe("online");
});

test("expired data is fetched again and key types remain distinct", async () => {
  vi.useFakeTimers();
  try {
    const client = createQueryClient();
    const queryFn = vi.fn().mockResolvedValueOnce("first").mockResolvedValueOnce("second");
    const options = { key: [1] as const, queryFn, staleTime: 1_000 };
    expect(await client.fetchQuery(options)).toBe("first");
    vi.advanceTimersByTime(1_000);
    expect(await client.fetchQuery(options)).toBe("second");

    client.setQueryData(["1"], "text");
    expect(client.getQueryData([1])).toBe("second");
    expect(client.getQueryData(["1"])).toBe("text");
  } finally {
    vi.useRealTimers();
  }
});

test("invalidation during a request keeps its result stale", async () => {
  const client = createQueryClient();
  const pending = Promise.withResolvers<string>();
  const queryFn = vi.fn().mockReturnValueOnce(pending.promise).mockResolvedValueOnce("fresh");
  const options = { key: ["post", "5"] as const, queryFn, staleTime: 10_000 };

  const loading = client.fetchQuery(options);
  client.invalidateQueries(options.key);
  pending.resolve("invalidated response");
  expect(await loading).toBe("invalidated response");
  expect(await client.fetchQuery(options)).toBe("fresh");
  expect(queryFn).toHaveBeenCalledTimes(2);
});

test("inactive results are collected after their retention window", () => {
  vi.useFakeTimers();
  try {
    const client = createQueryClient({ gcTime: 1_000 });
    client.setQueryData(["post", "6"], "data");
    expect(client.getQueryData(["post", "6"])).toBe("data");

    vi.advanceTimersByTime(1_000);
    expect(client.getQueryData(["post", "6"])).toBeUndefined();
  } finally {
    vi.useRealTimers();
  }
});

test("set data wins over an older in-flight response", async () => {
  const client = createQueryClient();
  const pending = Promise.withResolvers<string>();
  const options = { key: ["post", "3"] as const, queryFn: () => pending.promise, staleTime: 10_000 };

  const loading = client.fetchQuery(options);
  client.setQueryData(options.key, "updated");
  pending.resolve("stale response");
  await loading;

  expect(client.getQueryData(options.key)).toBe("updated");
});

test("dehydrated data can be restored into an isolated client", async () => {
  const server = createQueryClient();
  await server.fetchQuery({ key: ["post", "4"], queryFn: () => ({ title: "Server" }), staleTime: 10_000 });

  const browser = createQueryClient();
  browser.hydrate(server.dehydrate());
  const queryFn = vi.fn(() => ({ title: "Browser" }));
  const options = { key: ["post", "4"] as const, queryFn, staleTime: 10_000 };

  expect(browser.getQueryData(options.key)).toEqual({ title: "Server" });
  expect(await browser.fetchQuery(options)).toEqual({ title: "Server" });
  expect(queryFn).not.toHaveBeenCalled();
  expect(createQueryClient().getQueryData(options.key)).toBeUndefined();
});

test("a request started before an invalidation is not reused", async () => {
  const client = createQueryClient({ staleTime: 10_000 });
  const first = Promise.withResolvers<string>();
  const queryFn = vi.fn().mockReturnValueOnce(first.promise).mockResolvedValueOnce("fresh");
  const options = { key: ["post", "7"] as const, queryFn };

  const before = client.fetchQuery(options);
  await Promise.resolve();
  client.invalidateQueries(options.key);
  const after = client.fetchQuery(options);
  first.resolve("old");

  expect(await after).toBe("fresh");
  expect(await before).toBe("old");
  expect(client.getQueryData(options.key)).toBe("fresh");
  expect(queryFn).toHaveBeenCalledTimes(2);
});

test("callers of a replaced request receive the replacement instead of an abort", async () => {
  const client = createQueryClient();
  const queryFn = vi.fn(
    ({ signal }: { signal: AbortSignal }) =>
      new Promise<string>((_, reject) => {
        if (signal.aborted) reject(signal.reason);
        signal.addEventListener("abort", () => reject(signal.reason));
      }),
  );

  const replacedBeforeStart = client.fetchQuery({ key: ["post", "8"], queryFn });
  client.setQueryData(["post", "8"], "manual");
  expect(await replacedBeforeStart).toBe("manual");
  expect(queryFn).not.toHaveBeenCalled();

  const replacedInFlight = client.fetchQuery({ key: ["post", "9"], queryFn });
  await Promise.resolve();
  await Promise.resolve();
  client.setQueryData(["post", "9"], "manual");
  expect(await replacedInFlight).toBe("manual");

  const cleared = client.fetchQuery({ key: ["post", "10"], queryFn });
  client.clear();
  await expect(cleared).rejects.toMatchObject({ name: "AbortError" });
});

test("hydrated age ignores clock differences and invalid state restores nothing", () => {
  vi.useFakeTimers();
  try {
    const server = createQueryClient({ staleTime: 60_000 });
    vi.setSystemTime(new Date(2_000_000));
    server.setQueryData(["post", "11"], "server");
    vi.setSystemTime(new Date(2_010_000));
    const state = server.dehydrate();
    expect(state[0]?.ageMs).toBe(10_000);

    vi.setSystemTime(new Date(9_000_000));
    const browser = createQueryClient({ staleTime: 60_000 });
    browser.hydrate(state);
    const entry = state[0]!;
    expect(() => browser.hydrate([{ ...entry, key: ["post", "12"] }, { ...entry, ageMs: -1 }])).toThrow(TypeError);
    expect(browser.getQueryData(["post", "12"])).toBeUndefined();
    expect(browser.getQueryData(["post", "11"])).toBe("server");
  } finally {
    vi.useRealTimers();
  }
});

test("invalid options reject and infinite durations never expire", async () => {
  const client = createQueryClient({ staleTime: Infinity, gcTime: Infinity });
  await expect(client.fetchQuery({ key: [Number.NaN], queryFn: () => 1 })).rejects.toThrow(TypeError);
  await expect(client.fetchQuery({ key: ["a"], queryFn: () => 1, staleTime: -1 })).rejects.toThrow(RangeError);

  vi.useFakeTimers();
  try {
    const queryFn = vi.fn(() => "kept");
    expect(await client.fetchQuery({ key: ["post", "13"], queryFn })).toBe("kept");
    vi.advanceTimersByTime(365 * 24 * 60 * 60_000);
    expect(await client.fetchQuery({ key: ["post", "13"], queryFn })).toBe("kept");
    expect(queryFn).toHaveBeenCalledTimes(1);
  } finally {
    vi.useRealTimers();
  }
});
