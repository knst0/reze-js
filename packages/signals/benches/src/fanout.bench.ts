import { effect, flush, root, signal } from "@rezejs/signals";
import { test } from "vite-plus/test";

import { measure } from "./_baseline";

const EFFECTS = 100;
const ITERS = 100;

test("fanout", async (context) => {
  await root(async (dispose) => {
    const [get, set] = signal(0);
    for (let i = 0; i < EFFECTS; i++) {
      effect(() => {
        get();
      });
    }
    let next = 0;
    await measure(context, `1 write -> ${EFFECTS} effects`, () => {
      for (let i = 0; i < ITERS; i++) {
        next += 1;
        set(next);
        flush();
      }
    });
    dispose();
  });
});
