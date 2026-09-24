import { expect, test } from "vitest";

import { effect, flush, root, selector, signal } from "../src";

function watchRows(count: number, isSelected: (key: number) => boolean) {
  const runs: number[] = Array.from({ length: count }, () => 0);
  const disposers: (() => void)[] = [];
  for (let key = 0; key < count; key++) {
    disposers.push(
      effect(() => {
        isSelected(key);
        runs[key]!++;
      }),
    );
  }
  return { runs, disposers };
}

test("a change re-runs only the previous and the next key's subscribers", () => {
  root(() => {
    const [selected, setSelected] = signal(1);
    const isSelected = selector(selected);
    const { runs } = watchRows(1000, isSelected);
    runs.fill(0);

    setSelected(2);
    flush();

    expect(runs.reduce((sum, n) => sum + n, 0)).toBe(2);
    expect(runs[1]).toBe(1);
    expect(runs[2]).toBe(1);
    expect(isSelected(2)).toBe(true);
    expect(isSelected(1)).toBe(false);
  });
});

test("reports the selection state to each caller", () => {
  root(() => {
    const [selected, setSelected] = signal("a");
    const isSelected = selector(selected);
    const seen: Record<string, boolean> = {};
    for (const key of ["a", "b", "c"]) effect(() => void (seen[key] = isSelected(key)));
    expect(seen).toEqual({ a: true, b: false, c: false });

    setSelected("c");
    flush();
    expect(seen).toEqual({ a: false, b: false, c: true });
  });
});

test("keys without subscribers are dropped", () => {
  root(() => {
    const [selected, setSelected] = signal(0);
    const isSelected = selector(selected);
    const { disposers, runs } = watchRows(10, isSelected);
    for (const dispose of disposers) dispose();
    runs.fill(0);

    setSelected(5);
    flush();

    expect(runs.every((n) => n === 0)).toBe(true);
    let reads = 0;
    effect(() => {
      isSelected(5);
      reads++;
    });
    expect(reads).toBe(1);
    expect(isSelected(5)).toBe(true);
  });
});

test("writes settle to the last value", () => {
  root(() => {
    const [selected, setSelected] = signal(0);
    const isSelected = selector(selected);
    const { runs } = watchRows(5, isSelected);
    runs.fill(0);

    setSelected(1);
    setSelected(2);
    setSelected(3);
    flush();

    expect(isSelected(3)).toBe(true);
    expect(runs).toEqual([1, 0, 0, 1, 0]);
  });
});

test("a key subscribed in the same flush as a source change sees the new value", () => {
  root(() => {
    const [selected, setSelected] = signal(0);
    const [keys, setKeys] = signal([0]);
    const isSelected = selector(selected);
    const seen = new Map<number, boolean>();
    effect(() => {
      for (const key of keys()) effect(() => void seen.set(key, isSelected(key)));
    });

    setKeys([0, 7]);
    setSelected(7);
    flush();

    expect(seen.get(7)).toBe(true);
    expect(seen.get(0)).toBe(false);
  });
});

test("a custom equals re-checks every live key", () => {
  root(() => {
    const [threshold, setThreshold] = signal(5);
    const isSelected = selector(threshold, (key, value) => key >= value);
    const seen: boolean[] = [];
    for (let key = 0; key < 10; key++) effect(() => void (seen[key] = isSelected(key)));
    expect(seen.filter(Boolean)).toHaveLength(5);

    setThreshold(8);
    flush();
    expect(seen.filter(Boolean)).toHaveLength(2);
  });
});

test("reads outside a tracking context do not create key nodes", () => {
  root(() => {
    const [selected, setSelected] = signal(1);
    const isSelected = selector(selected);
    expect(isSelected(1)).toBe(true);
    setSelected(2);
    flush();
    expect(isSelected(1)).toBe(false);
    expect(isSelected(2)).toBe(true);
  });
});

test("stops following the source once its owner is disposed", () => {
  const [selected, setSelected] = signal(1);
  let isSelected!: (key: number) => boolean;
  const dispose = root((d) => {
    isSelected = selector(selected);
    return d;
  });
  dispose();
  setSelected(2);
  flush();
  expect(isSelected(2)).toBe(false);
});
