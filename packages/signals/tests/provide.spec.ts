import { expect, test } from "vitest";

import { computed, effect, flush, provideContext, root, signal, untrack, useContext, type ContextKey } from "../src";

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
      flush();
      seen.push(read());
    });
  });
  expect(seen).toEqual(["dark", "dark", "dark", "dark", "dark", "dark", "dark"]);
});
