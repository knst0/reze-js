import { expect, test } from "vite-plus/test";

import { computed, effect, signal, trigger } from "../src";

test("trigger updates dependent computeds, and reruns an effect once when a write follows a read of the same signal", () => {
  const [arr] = signal<number[]>([]);
  const length = computed(() => arr().length);
  expect(length()).toBe(0);
  arr().push(1);
  trigger(arr);
  expect(length()).toBe(1);

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
