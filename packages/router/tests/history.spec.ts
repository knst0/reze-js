import { expect, test } from "vitest";

import { memoryHistory } from "../src";

test("pushes, replaces and clamps traversal", () => {
  const history = memoryHistory("/a");
  const seen: string[] = [];
  const stop = history.listen((value) => void seen.push(value));
  expect(history.get()).toBe("/a");
  history.set({ value: "/b" });
  history.set({ value: "/c", replace: true });
  expect(history.get()).toBe("/c");
  expect(seen).toEqual(["/b", "/c"]);
  history.back();
  expect(history.get()).toBe("/a");
  history.forward();
  expect(history.get()).toBe("/c");
  history.go(-10);
  expect(history.get()).toBe("/a");
  history.go(10);
  expect(history.get()).toBe("/c");
  stop();
  history.set({ value: "/d" });
  expect(seen).toEqual(["/b", "/c", "/a", "/c", "/a", "/c"]);
});

test("pushing drops the forward entries", () => {
  const history = memoryHistory("/");
  history.set({ value: "/a" });
  history.set({ value: "/b" });
  history.back();
  history.set({ value: "/c" });
  expect(history.get()).toBe("/c");
  history.forward();
  expect(history.get()).toBe("/c");
});
