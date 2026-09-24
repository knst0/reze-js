import { expect, test } from "vitest";

import { effect, flushSync, store } from "../src";

function runs(read: () => unknown): { count: number } {
  const counter = { count: 0 };
  effect(() => {
    read();
    counter.count++;
  });
  return counter;
}

test("reading a nested leaf re-runs only when that leaf changes", () => {
  const [state, setState] = store({ user: { name: "a", age: 1 }, tags: ["x"] });
  const name = runs(() => state.user.name);
  const age = runs(() => state.user.age);
  setState((d) => {
    d.user.name = "b";
  });
  flushSync();
  expect([name.count, age.count]).toEqual([2, 1]);
  expect(state.user.name).toBe("b");
});

test("writing an equal value does not notify, and NaN equals itself", () => {
  const [state, setState] = store({ n: NaN, s: "a" });
  const reads = runs(() => [state.n, state.s]);
  setState((d) => {
    d.n = NaN;
    d.s = "a";
  });
  flushSync();
  expect(reads.count).toBe(1);
});

test("key set and length are tracked on their own", () => {
  const [state, setState] = store<{ list: number[]; map: Record<string, number> }>({
    list: [1, 2],
    map: { a: 1 },
  });
  const length = runs(() => state.list.length);
  const keys = runs(() => Object.keys(state.map));
  const hasB = runs(() => "b" in state.map);
  const first = runs(() => state.list[0]);

  setState((d) => {
    d.map.a = 2;
  });
  flushSync();
  expect([keys.count, hasB.count]).toEqual([1, 1]);

  setState((d) => {
    d.list.push(3);
    d.map.b = 1;
  });
  flushSync();
  expect([length.count, keys.count, hasB.count, first.count]).toEqual([2, 2, 2, 1]);

  setState((d) => {
    delete d.map.a;
    d.list.length = 0;
  });
  flushSync();
  expect([length.count, keys.count, first.count]).toEqual([3, 3, 2]);
  expect(state.list[0]).toBeUndefined();
  expect(Object.keys(state.map)).toEqual(["b"]);
});

test("reading a missing key tracks its later creation", () => {
  const [state, setState] = store<{ a?: number }>({});
  const reads = runs(() => state.a);
  setState((d) => {
    d.a = 1;
  });
  flushSync();
  expect(reads.count).toBe(2);
  expect(state.a).toBe(1);
});

test("state rejects writes, deletes and property definitions at any depth", () => {
  const [state] = store({ nested: { a: 1 } as { a?: number } });
  expect(() => {
    (state.nested as { a: number }).a = 2;
  }).toThrow(TypeError);
  expect(() => delete state.nested.a).toThrow(TypeError);
  expect(() => Object.defineProperty(state, "x", { value: 1 })).toThrow(TypeError);
  expect(state.nested.a).toBe(1);
});

test("a draft throws on any access after setState returns", () => {
  const [, setState] = store({ nested: { a: 1 } });
  let root!: { nested: { a: number } };
  let nested!: { a: number };
  setState((d) => {
    root = d;
    nested = d.nested;
  });
  expect(() => root.nested).toThrow(TypeError);
  expect(() => nested.a).toThrow(TypeError);
  expect(() => {
    nested.a = 2;
  }).toThrow(TypeError);
});

test("setState runs untracked: reads inside do not subscribe the caller", () => {
  const [state, setState] = store({ a: 0, b: 0 });
  const reads = runs(() =>
    setState((d) => {
      d.b = d.a + state.a;
    }),
  );
  setState((d) => {
    d.a = 1;
  });
  flushSync();
  expect(reads.count).toBe(1);
  expect(state.b).toBe(0);
});

test("setState returns undefined and a draft reads its own writes", () => {
  const [state, setState] = store({ count: 1 });
  let seen = 0;
  const result = setState((d) => {
    d.count++;
    seen = d.count;
  });
  expect(result).toBeUndefined();
  expect(seen).toBe(2);
  expect(state.count).toBe(2);
});

test("written objects join the tree and are reactive", () => {
  const [state, setState] = store<{ item: { label: string } | null }>({ item: null });
  const label = runs(() => state.item?.label);
  setState((d) => {
    d.item = { label: "a" };
  });
  flushSync();
  setState((d) => {
    d.item!.label = "b";
  });
  flushSync();
  expect(label.count).toBe(3);
  expect(state.item!.label).toBe("b");
});

test("storing a draft stores its object, not the revocable proxy", () => {
  const [state, setState] = store({ a: { v: 1 }, b: null as { v: number } | null });
  setState((d) => {
    d.b = d.a;
  });
  expect(state.b).toBe(state.a);
  expect(state.b!.v).toBe(1);
});

test("init is adopted, not copied", () => {
  const init = { list: [1] };
  const [, setState] = store(init);
  setState((d) => {
    d.list.push(2);
  });
  expect(init.list).toEqual([1, 2]);
});

test("function values are stored as values", () => {
  const fn = () => 1;
  const [state, setState] = store<{ fn: (() => number) | null }>({ fn: null });
  setState((d) => {
    d.fn = fn;
  });
  expect(state.fn).toBe(fn);
});
