import { afterEach, beforeEach, expect, test, vi, type MockInstance } from "vitest";

import { catchError, flush, root, signal } from "../src/index";
import { fixedRenderEffect } from "../src/render";

let warn: MockInstance<typeof console.warn>;

beforeEach(() => {
  warn = vi.spyOn(console, "warn").mockImplementation(() => {});
});

afterEach(() => {
  warn.mockRestore();
});

test("reruns on every write of its dependency without warning", () => {
  root((dispose) => {
    const [a, setA] = signal(0);
    const out: number[] = [];
    fixedRenderEffect(() => {
      out.push(a());
    });
    for (let i = 1; i <= 3; i++) {
      setA(i);
      flush();
    }
    expect(out).toEqual([0, 1, 2, 3]);
    expect(warn).not.toHaveBeenCalled();
    dispose();
  });
});

test("coalesces writes to several dependencies into one rerun", () => {
  root((dispose) => {
    const [a, setA] = signal(1);
    const [b, setB] = signal(10);
    const out: number[] = [];
    fixedRenderEffect(() => {
      out.push(a() + b());
    });
    setA(2);
    setB(20);
    flush();
    expect(out).toEqual([11, 22]);
    dispose();
  });
});

test("development warns once when a rerun reads a different set, and keeps tracking", () => {
  root((dispose) => {
    const [flag, setFlag] = signal(true);
    const [a] = signal("a");
    const [b, setB] = signal("b");
    const out: string[] = [];
    fixedRenderEffect(() => {
      out.push(flag() ? a() : b());
    });
    setFlag(false);
    flush();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0]![0]).toContain("[rezejs] A compiled binding read a different set");
    expect(warn.mock.calls[0]![0]).toMatch(/Binding: .*flag\(\)/s);
    setB("b2");
    flush();
    expect(out).toEqual(["a", "b", "b2"]);
    setFlag(true);
    flush();
    expect(warn).toHaveBeenCalledTimes(1);
    dispose();
  });
});

test("an error thrown by a rerun reaches the enclosing catchError", () => {
  root((dispose) => {
    const [a, setA] = signal(0);
    const errors: unknown[] = [];
    catchError(
      () => {
        fixedRenderEffect(() => {
          if (a() === 1) throw new Error("boom");
        });
      },
      (error) => errors.push(error),
    );
    setA(1);
    flush();
    expect(errors).toHaveLength(1);
    expect((errors[0] as Error).message).toBe("boom");
    dispose();
  });
});
