import { testSuite, SkipTest, setExpect, type ReactiveFramework } from "reactive-framework-test-suite";
import { describe, expect, test } from "vite-plus/test";

import { computed, effect, effectScope, flush, signal, untrack } from "../src";

const framework: ReactiveFramework = {
  signal(initialValue) {
    const [read, write] = signal(initialValue);
    return {
      read,
      write: (v) => {
        write(() => v);
        flush();
      },
    };
  },
  computed(fn) {
    const c = computed(fn);
    return { read: () => c() };
  },
  effect(fn) {
    return effect(fn);
  },
  run(fn) {
    effectScope(fn)();
  },
  untracked: untrack,
};

setExpect(expect);

for (const { section, cases } of testSuite) {
  describe(section, () => {
    for (const [name, fn] of Object.entries(cases)) {
      test(name, () => {
        try {
          framework.run(() => fn(framework));
        } catch (e) {
          if (e instanceof SkipTest) return;
          throw e;
        }
      });
    }
  });
}
