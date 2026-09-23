import {
  testSuite,
  SkipTest,
  setExpect,
  type ReactiveFramework,
} from "reactive-framework-test-suite";
import { describe, expect, test } from "vitest";

import { signal, computed, effect, effectScope } from "../src";
import { setActiveSub } from "../src/context";
import { endBatch, startBatch } from "../src/scheduler";

const framework: ReactiveFramework = {
  signal(initialValue) {
    const [read, write] = signal(initialValue);
    return {
      read,
      write: (v) => {
        write(() => v);
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
  batch(fn) {
    startBatch();
    try {
      fn();
    } finally {
      endBatch();
    }
  },
  untracked(fn) {
    const prev = setActiveSub(undefined);
    try {
      return fn();
    } finally {
      setActiveSub(prev);
    }
  },
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
