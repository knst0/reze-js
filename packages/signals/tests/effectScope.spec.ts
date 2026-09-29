import { expect, test } from "vitest";

import { effect, effectScope, flush, signal } from "../src";

test("scope as intermediate parent: cleanup order respects nesting", () => {
  // When effectScope is used as an intermediate scope inside an outer
  // effect, the outer's re-run must still dispose the scope (and its
  // effects) before running the outer's own cleanup.
  const [a, setA] = signal(0);
  const log: string[] = [];

  effect(() => {
    a();
    log.push("outer:run");
    effectScope(() => {
      effect(() => {
        log.push("inner:run");
        return () => log.push("inner:cleanup");
      });
    });
    return () => log.push("outer:cleanup");
  });
  log.length = 0;

  setA(1);
  flush();
  expect(log).toEqual(["inner:cleanup", "outer:cleanup", "outer:run", "inner:run"]);
});
