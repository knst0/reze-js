import { expect, test } from "vitest";

import { computed, effect, flushSync, signal } from "../src";

test("setter applies updater functions to the latest value and returns the result", () => {
  const [count, setCount] = signal(1);
  expect(setCount((n) => n + 1)).toBe(2);
  expect(setCount((n) => n * 10)).toBe(20);
  expect(count()).toBe(20);
});

test("a function value is stored by wrapping it in an updater", () => {
  const fn = () => 1;
  const [get, set] = signal<() => number>(() => 0);
  set(() => fn);
  expect(get()).toBe(fn);
});

test("default equality is Object.is: NaN writes do not notify", () => {
  const [n, setN] = signal(NaN);
  let runs = 0;
  effect(() => {
    n();
    runs++;
  });
  setN(NaN);
  flushSync();
  expect(runs).toBe(1);
});

test("default equality is Object.is: +0 and -0 notify each other (P06)", () => {
  const [n, setN] = signal(0);
  let runs = 0;
  effect(() => {
    n();
    runs++;
  });
  flushSync();
  setN(-0);
  flushSync();
  expect(runs).toBe(2);
  setN(-0);
  flushSync();
  expect(runs).toBe(2);
  setN(0);
  flushSync();
  expect(runs).toBe(3);
});

test("equals: false notifies on every write, even with the same value", () => {
  const [items, setItems] = signal<number[]>([], { equals: false });
  const length = computed(() => items().length);
  let runs = 0;
  effect(() => {
    items();
    runs++;
  });
  const arr = items();
  arr.push(1);
  setItems(arr);
  flushSync();
  expect(runs).toBe(2);
  expect(length()).toBe(1);
});

test("custom equals suppresses notification when it reports equality", () => {
  const [point, setPoint] = signal(
    { x: 0, y: 0 },
    { equals: (a, b) => a.x === b.x && a.y === b.y },
  );
  let runs = 0;
  effect(() => {
    point();
    runs++;
  });
  setPoint({ x: 0, y: 0 });
  flushSync();
  expect(runs).toBe(1);
  setPoint({ x: 1, y: 0 });
  flushSync();
  expect(runs).toBe(2);
});

test("ReadonlySignal names the getter half for read-only consumers (D08)", () => {
  const [count] = signal(1);
  const read: import("../src").ReadonlySignal<number> = count;
  expect(read()).toBe(1);
});
