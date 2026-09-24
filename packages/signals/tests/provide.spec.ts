import { expect, test } from "vitest";

import {
  computed,
  effect,
  flushSync,
  provideContext,
  root,
  runWithOwner,
  getOwner,
  signal,
  untrack,
  useContext,
  type ContextKey,
} from "../src";

const Theme: ContextKey<string> = { id: Symbol("theme"), defaultValue: "light" };

test("the default applies without a provider", () => {
  expect(useContext(Theme)).toBe("light");
  root(() => expect(useContext(Theme)).toBe("light"));
});

test("the nearest provider wins", () => {
  root(() => {
    provideContext(Theme, "dark", () => {
      expect(useContext(Theme)).toBe("dark");
      provideContext(Theme, "blue", () => expect(useContext(Theme)).toBe("blue"));
      expect(useContext(Theme)).toBe("dark");
    });
  });
});

test("an explicit undefined value is provided, not the default", () => {
  const Maybe: ContextKey<string | undefined> = { id: Symbol(), defaultValue: "fallback" };
  root(() => provideContext(Maybe, undefined, () => expect(useContext(Maybe)).toBeUndefined()));
});

test("lookups walk through effects, computeds, nested roots and untrack", () => {
  const seen: string[] = [];
  root(() => {
    provideContext(Theme, "dark", () => {
      const [n, setN] = signal(0);
      effect(() => {
        n();
        seen.push(useContext(Theme));
        root(() => seen.push(useContext(Theme)));
      });
      const read = computed(() => (n(), useContext(Theme)));
      seen.push(read());
      effect(() => void untrack(() => seen.push(useContext(Theme))));
      setN(1);
      flushSync();
      seen.push(read());
    });
  });
  expect(seen).toEqual(["dark", "dark", "dark", "dark", "dark", "dark", "dark"]);
});

test("work resumed with runWithOwner sees the provider", () => {
  let owner: ReturnType<typeof getOwner>;
  root(() => provideContext(Theme, "dark", () => (owner = getOwner())));
  runWithOwner(owner, () => expect(useContext(Theme)).toBe("dark"));
});

test("a provider is disposed with its owner", () => {
  let cleaned = 0;
  const dispose = root((d) => {
    provideContext(Theme, "dark", () => {
      effect(() => () => cleaned++);
    });
    return d;
  });
  dispose();
  expect(cleaned).toBe(1);
});
