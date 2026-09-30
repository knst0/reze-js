import { expect, expectTypeOf, test } from "vitest";

import { $signal } from "../src";

test("`$signal` without the compiler throws instead of returning an inert value", () => {
  expect(() => $signal(0)).toThrow("requires the reze compiler");
});

test("`$signal` is typed as its value, so a getter call is a type error", () => {
  const types = () => {
    const count = $signal(0);
    expectTypeOf(count).toEqualTypeOf<number>();
    expectTypeOf($signal<string>()).toEqualTypeOf<string | undefined>();
    expectTypeOf($signal("a", { equals: false, name: "a" })).toEqualTypeOf<string>();
    // @ts-expect-error a `$signal` variable is the value, not a getter
    count();
  };
  expect(types).toThrow("requires the reze compiler");
});
