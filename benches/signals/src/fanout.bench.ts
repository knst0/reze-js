import { effect, flushSync, root, signal } from "@rezejs/signals";
import { test } from "vitest";

import { logResult } from "./_log";

const EFFECTS = 100;
const ITERS = 100;

// One signal fanning out to many effects (list-row update shape).
test("fanout", async ({ bench }) => {
  await root(async (dispose) => {
    const [get, set] = signal(0);
    for (let i = 0; i < EFFECTS; i++) {
      effect(() => {
        get();
      });
    }
    let next = 0;
    logResult(
      "fanout",
      await bench(`1 write -> ${EFFECTS} effects`, () => {
        for (let i = 0; i < ITERS; i++) {
          next += 1;
          set(next);
          flushSync();
        }
      }).run(),
    );
    dispose();
  });
});
