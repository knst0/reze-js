import { computed, effect, flush, root, signal } from "@rezejs/signals";
import { test } from "vitest";

import { measure } from "./_baseline";

const ITERS = 1000;

test("counter path", async (context) => {
  await root(async (dispose) => {
    const [count, setCount] = signal(0);
    const doubled = computed(() => count() * 2);
    effect(() => {
      count();
      doubled();
    });
    let next = 0;
    await measure(context, "click-like writes", () => {
      for (let i = 0; i < ITERS; i++) {
        next += 1;
        setCount(next);
        flush();
      }
    });
    await measure(context, "coalesced writes", () => {
      for (let i = 0; i < ITERS; i++) {
        next += 1;
        setCount(next);
      }
      flush();
    });
    dispose();
  });
});
