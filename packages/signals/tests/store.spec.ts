import { expect, test } from "vite-plus/test";

import { computed, effect, flush, readonly, store } from "../src";
import { renderEffect } from "../src/render";

function runs(read: () => unknown): { count: number } {
  const counter = { count: 0 };
  effect(() => {
    read();
    counter.count++;
  });
  return counter;
}

test("reading a nested leaf re-runs only when that leaf changes", () => {
  const state = store({ user: { name: "a", age: 1 }, tags: ["x"] });
  const name = runs(() => state.user.name);
  const age = runs(() => state.user.age);
  state.user.name = "b";
  flush();
  expect([name.count, age.count]).toEqual([2, 1]);
  expect(state.user.name).toBe("b");
});

test("writing an equal value does not notify, and NaN equals itself", () => {
  const state = store({ n: NaN, s: "a" });
  const reads = runs(() => [state.n, state.s]);
  state.n = NaN;
  state.s = "a";
  flush();
  expect(reads.count).toBe(1);
});

test("key set and length are tracked on their own", () => {
  const state = store<{ list: number[]; map: Record<string, number> }>({
    list: [1, 2],
    map: { a: 1 },
  });
  const length = runs(() => state.list.length);
  const keys = runs(() => Object.keys(state.map));
  const hasB = runs(() => "b" in state.map);
  const first = runs(() => state.list[0]);

  state.map.a = 2;
  flush();
  expect([keys.count, hasB.count]).toEqual([1, 1]);

  state.list.push(3);
  state.map.b = 1;
  flush();
  expect([length.count, keys.count, hasB.count, first.count]).toEqual([2, 2, 2, 1]);

  delete state.map.a;
  state.list.length = 0;
  flush();
  expect([length.count, keys.count, first.count]).toEqual([3, 3, 2]);
  expect(state.list[0]).toBeUndefined();
  expect(Object.keys(state.map)).toEqual(["b"]);
});

test("reading a missing key tracks its later creation", () => {
  const state = store<{ a?: number }>({});
  const reads = runs(() => state.a);
  state.a = 1;
  flush();
  expect(reads.count).toBe(2);
  expect(state.a).toBe(1);
});

test("a write is visible to the next read at once, and effects wait for the flush", () => {
  const state = store({ a: 0 });
  const reads = runs(() => state.a);
  state.a = 1;
  expect(state.a).toBe(1);
  expect(reads.count).toBe(1);
  flush();
  expect(reads.count).toBe(2);
});

test("written objects join the tree and are reactive", () => {
  const state = store<{ item: { label: string } | null }>({ item: null });
  const label = runs(() => state.item?.label);
  state.item = { label: "a" };
  flush();
  state.item!.label = "b";
  flush();
  expect(label.count).toBe(3);
  expect(state.item!.label).toBe("b");
});

test("storing a proxy stores its object, so both paths reach the same signals", () => {
  const state = store({ a: { v: 1 }, b: null as { v: number } | null });
  state.b = state.a;
  expect(state.b).toBe(state.a);
  const reads = runs(() => state.a.v);
  state.b!.v = 2;
  flush();
  expect(reads.count).toBe(2);
});

test("readonly reads the same signals and rejects writes, deletes and definitions at any depth", () => {
  const state = store<{ nested: { a?: number } }>({ nested: { a: 1 } });
  const view = readonly(state);
  const reads = runs(() => view.nested.a);
  expect(() => {
    view.nested.a = 2;
  }).toThrow(TypeError);
  expect(() => delete view.nested.a).toThrow(TypeError);
  expect(() => Object.defineProperty(view, "x", { value: 1 })).toThrow(TypeError);
  state.nested.a = 3;
  flush();
  expect(reads.count).toBe(2);
  expect(view.nested.a).toBe(3);
});

test("a write while a computed or render binding runs throws in development", () => {
  const state = store<{ a?: number; b: number }>({ a: 0, b: 0 });
  const derived = computed(() => (state.b = (state.a ?? 0) + 1));
  expect(derived).toThrow(TypeError);
  expect(() =>
    renderEffect(() => {
      delete state.a;
    }),
  ).toThrow(TypeError);
  expect(state).toEqual({ a: 0, b: 0 });
});
