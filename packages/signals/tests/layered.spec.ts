import { expect, test } from "vitest";

import { effect, flush, layeredSignal } from "../src";

function observe<T>(read: () => T): T[] {
  const seen: T[] = [];
  effect(() => {
    seen.push(read());
  });
  flush();
  return seen;
}

test("a layer shows at once and removing it restores the base", () => {
  const [likes, , layer] = layeredSignal(10);
  const seen = observe(likes);

  const drop = layer((n) => n + 1);
  flush();
  expect(likes()).toBe(11);

  drop();
  flush();
  expect(likes()).toBe(10);
  expect(seen).toEqual([10, 11, 10]);
});

test("removing an earlier layer keeps the later one over the base", () => {
  const [items, , layer] = layeredSignal<string[]>([]);
  const dropFirst = layer((list) => [...list, "first"]);
  layer((list) => [...list, "second"]);
  expect(items()).toEqual(["first", "second"]);

  dropFirst();
  expect(items()).toEqual(["second"]);
});

test("the setter writes the base and its updater never sees the shown value", () => {
  const [count, setCount, layer] = layeredSignal(0);
  layer((n) => n + 100);

  expect(setCount((n) => n + 1)).toBe(1);
  expect(count()).toBe(101);
});

test("confirming the base and removing the layer in one task notifies once with the final value", () => {
  const [count, setCount, layer] = layeredSignal(0);
  const drop = layer((n) => n + 1);
  const seen = observe(count);

  setCount(1);
  drop();
  flush();

  expect(seen).toEqual([1]);
});

test("removing the same layer twice never removes an identical layer added later", () => {
  const [count, , layer] = layeredSignal(0);
  const increment = (n: number): number => n + 1;
  const drop = layer(increment);
  layer(increment);

  drop();
  drop();
  expect(count()).toBe(1);
});

test("a throwing layer leaves the signal and its layers unchanged", () => {
  const [count, setCount, layer] = layeredSignal(1);
  layer((n) => n * 10);

  expect(() =>
    layer((n) => {
      if (n >= 10) throw new Error("rejected");
      return n;
    }),
  ).toThrow("rejected");
  expect(count()).toBe(10);

  setCount(2);
  expect(count()).toBe(20);
});
