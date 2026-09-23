import { signal } from "@rezejs/signals";
import { test } from "vitest";

import { logResult } from "./_log";

const ITERS = 1000;

test("raw signal", async ({ bench }) => {
  const [get, set] = signal(0);
  let next = 0;
  logResult(
    "raw signal",
    await bench("write+read, no subscribers", () => {
      for (let i = 0; i < ITERS; i++) {
        next += 1;
        set(next);
        get();
      }
    }).run(),
  );
});
