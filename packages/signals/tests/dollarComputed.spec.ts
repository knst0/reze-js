import { expect, expectTypeOf, test } from "vitest";

import { $computed } from "../src";

test("`$computed` without the compiler throws instead of returning an inert value", () => {
  expect(() => $computed(1)).toThrow("requires the reze compiler");
});

test("`$computed` is typed as its value, so a getter call is a type error", () => {
  const types = () => {
    const doubled = $computed(1);
    expectTypeOf(doubled).toEqualTypeOf<number>();
    expectTypeOf($computed("a", { name: "a" })).toEqualTypeOf<string>();
    // @ts-expect-error a `$computed` variable is the value, not a getter
    doubled();
  };
  expect(types).toThrow("requires the reze compiler");
});
