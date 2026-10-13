import { expect, test } from "vite-plus/test";

import { createSignalFrame, runWithSignalFrame, signal } from "../src";

test("a frame reuses signals by declaration order and ignores new initializers", () => {
  const frame = createSignalFrame();
  const first = runWithSignalFrame(frame, () => {
    const [count, setCount] = signal(1);
    const [label] = signal("a");
    setCount(42);
    return { count, setCount, label };
  });
  expect(first.count()).toBe(42);

  const second = runWithSignalFrame(frame, () => {
    const [count] = signal(0);
    const [label] = signal("changed");
    return { count, label };
  });
  expect(second.count()).toBe(42);
  first.setCount(43);
  expect(second.count()).toBe(43);
  expect(second.label()).toBe("a");
});

test("a shape mismatch resets only that position", () => {
  const frame = createSignalFrame();
  runWithSignalFrame(frame, () => {
    const [, setCount] = signal(1);
    const [, setLabel] = signal("a");
    const [, setItems] = signal<number[]>([1]);
    const [, setValue] = signal<string | null>("x");
    setCount(42);
    setLabel("kept");
    setItems([9]);
    setValue(null);
  });

  const rerun = runWithSignalFrame(frame, () => {
    const [count] = signal("reset");
    const [label] = signal("default");
    const [items] = signal({ length: 0 });
    const [value] = signal({});
    return { count, label, items, value };
  });
  expect(rerun.count()).toBe("reset");
  expect(rerun.label()).toBe("kept");
  expect(rerun.items()).toEqual({ length: 0 });
  expect(rerun.value()).toEqual({});
});

test("nested frames keep independent slots and restore the outer frame", () => {
  const outer = createSignalFrame();
  const inner = createSignalFrame();

  const first = runWithSignalFrame(outer, () => {
    const [before, setBefore] = signal(1);
    const innerFirst = runWithSignalFrame(inner, () => {
      const [value, setValue] = signal(10);
      setValue(11);
      return value;
    });
    const [after, setAfter] = signal(2);
    setBefore(3);
    setAfter(4);
    return { before, innerFirst, after };
  });

  const second = runWithSignalFrame(outer, () => {
    const [before] = signal(100);
    const innerSecond = runWithSignalFrame(inner, () => signal(1000)[0]);
    const [after] = signal(200);
    return { before, innerSecond, after };
  });

  expect(first.innerFirst()).toBe(11);
  expect(second.innerSecond()).toBe(11);
  expect(second.before()).toBe(3);
  expect(second.after()).toBe(4);
});

test("a frame is restored after a throw and signals outside a frame stay independent", () => {
  const outer = createSignalFrame();
  const inner = createSignalFrame();
  runWithSignalFrame(outer, () => {
    const [, setBefore] = signal(1);
    setBefore(5);
    expect(() =>
      runWithSignalFrame(inner, () => {
        signal(10);
        throw new Error("boom");
      }),
    ).toThrow("boom");
    const [, setAfter] = signal(2);
    setAfter(6);
  });

  const rerun = runWithSignalFrame(outer, () => {
    const [before] = signal(0);
    const [after] = signal(0);
    return { before, after };
  });
  expect(rerun.before()).toBe(5);
  expect(rerun.after()).toBe(6);

  const [plainA] = signal(1);
  const [plainB] = signal(1);
  expect(plainA).not.toBe(plainB);
});
