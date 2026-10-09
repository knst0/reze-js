import { expect, test } from "vite-plus/test";

import { computed, effect, trigger, signal } from "../src";

test("should trigger updates for dependent computed signals", () => {
  const [arr] = signal<number[]>([]);
  const length = computed(() => arr().length);

  expect(length()).toBe(0);
  arr().push(1);
  trigger(arr);
  expect(length()).toBe(1);
});

test("should rerun effect once when writing a signal after reading it", () => {
  const [src1, setSrc1] = signal(1);

  let triggers = 0;

  effect(() => {
    triggers++;
    src1();
  });

  expect(triggers).toBe(1);
  trigger(() => {
    src1();
    setSrc1(src1() + 1);
  });
  expect(triggers).toBe(2);
  expect(src1()).toBe(2);
});
