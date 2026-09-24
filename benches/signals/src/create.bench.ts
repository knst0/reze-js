import { computed, effect, root, signal } from "@rezejs/signals";
import { test } from "vitest";

import { logResult } from "./_log";

const COUNT = 10_000;

test("create", async ({ bench }) => {
  logResult(
    "create",
    await bench(`${COUNT} signals`, () => {
      root((dispose) => {
        for (let i = 0; i < COUNT; i++) signal(i);
        dispose();
      });
    }).run(),
  );
  logResult(
    "create",
    await bench(`${COUNT} signal + computed + effect`, () => {
      root((dispose) => {
        for (let i = 0; i < COUNT; i++) {
          const [get] = signal(i);
          const doubled = computed(() => get() * 2);
          effect(() => void doubled());
        }
        dispose();
      });
    }).run(),
  );
  logResult(
    "create",
    await bench(`${COUNT} row roots with a signal`, () => {
      const disposers: (() => void)[] = [];
      for (let i = 0; i < COUNT; i++) {
        root((dispose) => {
          disposers.push(dispose);
          const [get] = signal(i);
          effect(() => void get());
        });
      }
      for (const dispose of disposers) dispose();
    }).run(),
  );
});
