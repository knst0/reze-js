import { batch, computed, effect, flushSync, root, signal } from "@rezejs/signals";
import { test } from "vitest";

import { logResult } from "./_log";

const ITERS = 1000;

// Counter shape: one signal -> one computed -> one effect (text-node update path).
test("counter path", async ({ bench }) => {
  await root(async (dispose) => {
    const [count, setCount] = signal(0);
    const doubled = computed(() => count() * 2);
    effect(() => {
      count();
      doubled();
    });
    let next = 0;
    logResult(
      "counter path",
      await bench("click-like writes", () => {
        for (let i = 0; i < ITERS; i++) {
          next += 1;
          setCount(next);
          flushSync();
        }
      }).run(),
    );
    logResult(
      "counter path",
      await bench("batched writes", () => {
        batch(() => {
          for (let i = 0; i < ITERS; i++) {
            next += 1;
            setCount(next);
          }
        });
      }).run(),
    );
    dispose();
  });
});
