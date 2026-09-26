import { signal } from "@rezejs/signals";
import { test } from "vitest";

import { measure } from "./_baseline";

const ITERS = 1000;

test("raw signal", async (context) => {
  const [get, set] = signal(0);
  let next = 0;
  await measure(context, "write+read, no subscribers", () => {
    for (let i = 0; i < ITERS; i++) {
      next += 1;
      set(next);
      get();
    }
  });
});
