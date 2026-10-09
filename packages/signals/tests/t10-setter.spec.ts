import { expect, test } from "vite-plus/test";

import { effect, flush, signal } from "../src";

test("T10 detached setters preserve identity, pending updates, and receiver independence", () => {
  const [read, set] = signal(1);
  const original = set;
  let observed = 0;
  effect(() => {
    observed = read();
  });
  expect(set.call({ pendingValue: 100 }, (n) => n + 1)).toBe(2);
  expect(set.apply(undefined, [(n) => n * 3])).toBe(6);
  flush();
  expect(observed).toBe(6);
  expect(read.call({ currentValue: 100 })).toBe(6);
  expect(set).toBe(original);
});

test("T10 suppressed writes still return the requested value and updater sees latest stored value", () => {
  const [read, set] = signal({ id: 1, label: "a" }, { equals: (a, b) => a.id === b.id });
  const equal = { id: 1, label: "b" };
  expect(set(equal)).toBe(equal);
  expect(read().label).toBe("a");
  expect(set((prev) => ({ id: 2, label: prev.label }))).toEqual({ id: 2, label: "a" });
});
