import { afterEach, expect, test, vi } from "vitest";

import { effect, flush, optimistic, root, signal } from "../src";

function observe<T>(read: () => T): T[] {
  const seen: T[] = [];
  effect(() => {
    seen.push(read());
  });
  flush();
  return seen;
}

function tick(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

afterEach(() => {
  vi.restoreAllMocks();
});

test("a layer shows at once over the source and removing it restores the source", () => {
  const [likes, layer] = optimistic(() => 10);
  const seen = observe(likes);

  const drop = layer((n) => n + 1);
  flush();
  expect(likes()).toBe(11);

  drop();
  flush();
  expect(likes()).toBe(10);
  expect(seen).toEqual([10, 11, 10]);
});

test("removing an earlier layer keeps the later one over the source", () => {
  const [items, layer] = optimistic<string[]>(() => []);
  const dropFirst = layer((list) => [...list, "first"]);
  layer((list) => [...list, "second"]);
  expect(items()).toEqual(["first", "second"]);

  dropFirst();
  expect(items()).toEqual(["second"]);
});

test("a source change re-applies the layers", () => {
  const [base, setBase] = signal(1);
  const [count, layer] = optimistic(base);
  layer((n) => n * 10);
  expect(count()).toBe(10);

  setBase(2);
  expect(count()).toBe(20);
});

test("a signal read inside a layer re-applies it when it changes", () => {
  const [extra, setExtra] = signal(1);
  const [count, layer] = optimistic(() => 0);
  layer((n) => n + extra());
  const seen = observe(count);

  setExtra(5);
  flush();
  expect(seen).toEqual([1, 5]);
});

test("a source change and a layer removal in one task notify once with the final value", () => {
  const [base, setBase] = signal(0);
  const [count, layer] = optimistic(base);
  const drop = layer((n) => n + 1);
  const seen = observe(count);

  setBase(5);
  drop();
  flush();

  expect(seen).toEqual([1, 5]);
});

test("removing the same layer twice never removes an identical layer added later", () => {
  const [count, layer] = optimistic(() => 0);
  const increment = (n: number): number => n + 1;
  const drop = layer(increment);
  layer(increment);

  drop();
  drop();
  expect(count()).toBe(1);
});

test("a throwing layer is skipped while the others apply", () => {
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const [count, layer] = optimistic(() => 1);
  layer((n) => n * 10);
  layer(() => {
    throw new Error("rejected");
  });
  layer((n) => n + 1);

  expect(count()).toBe(11);
  expect(warn).toHaveBeenCalled();
});

test("a layer added inside a root is removed when the root is disposed", () => {
  const [count, layer] = optimistic(() => 0);
  let dispose!: () => void;
  root((stop) => {
    dispose = stop;
    layer((n) => n + 1);
  });
  expect(count()).toBe(1);

  dispose();
  expect(count()).toBe(0);
});

test("a layer with until is removed once that promise settles", async () => {
  const [count, layer] = optimistic(() => 0);
  const success = Promise.withResolvers<void>();
  const failure = Promise.withResolvers<void>();
  layer((n) => n + 1, success.promise);
  layer((n) => n + 10, failure.promise);
  expect(count()).toBe(11);

  success.resolve();
  await tick();
  expect(count()).toBe(10);

  failure.reject(new Error("no"));
  await tick();
  expect(count()).toBe(0);
});
