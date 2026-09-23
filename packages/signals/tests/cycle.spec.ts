import { afterEach, expect, test, vi } from "vitest";

import { computed, signal } from "../src";

const warn = vi.spyOn(console, "warn").mockImplementation(() => {});

afterEach(() => {
  warn.mockClear();
});

test("warns when a computed reads itself during evaluation", () => {
  const a = computed((): number => (b() ?? 0) + 1);
  const b = computed((): number => a());

  expect(a()).toBe(1);
  expect(warn).toHaveBeenCalledWith(expect.stringContaining("Cycle detected"));
});

test("a cycle formed by a dynamic dependency warns and terminates instead of hanging", () => {
  const [flag, setFlag] = signal(false);
  const [s, setS] = signal(0);
  const a = computed((): number => b() + 1);
  const b = computed((): number => (flag() ? a() + s() : 0));

  expect(a()).toBe(1);
  setFlag(true);
  a();
  warn.mockClear();

  // In production this descent never terminates.
  setS(1);
  a();
  expect(warn).toHaveBeenCalledWith(
    expect.stringContaining("Cycle detected in computed dependencies"),
  );
});

test("shared dependencies reached through several paths are not reported", () => {
  const [s, setS] = signal(0);
  const x = computed(() => s() * 2);
  const left = computed(() => x() + 1);
  const right = computed(() => x() + left());
  const sum = computed(() => left() + right() + x());

  expect(sum()).toBe(2);
  setS(1);
  expect(sum()).toBe(10);
  expect(warn).not.toHaveBeenCalled();
});
