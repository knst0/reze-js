import { computed, effect, root, signal } from "@rezejs/signals";
import { test } from "vitest";

import { measure } from "./_baseline";

const COUNT = 10_000;

test("create", async (context) => {
  await measure(context, `${COUNT} signals`, () => {
    root((dispose) => {
      for (let i = 0; i < COUNT; i++) signal(i);
      dispose();
    });
  });
  await measure(context, `${COUNT} signal + computed + effect`, () => {
    root((dispose) => {
      for (let i = 0; i < COUNT; i++) {
        const [get] = signal(i);
        const doubled = computed(() => get() * 2);
        effect(() => void doubled());
      }
      dispose();
    });
  });
  await measure(context, `${COUNT} row roots with a signal`, () => {
    const disposers: (() => void)[] = [];
    for (let i = 0; i < COUNT; i++) {
      root((dispose) => {
        disposers.push(dispose);
        const [get] = signal(i);
        effect(() => void get());
      });
    }
    for (const dispose of disposers) dispose();
  });
});
