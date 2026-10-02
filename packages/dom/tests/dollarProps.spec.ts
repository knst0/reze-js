import { expect, test } from "vitest";

import { $props } from "../src";

test("`$props` without the compiler throws instead of returning an inert value", () => {
  expect(() => $props.merge({ a: 1 })).toThrow("requires the reze compiler");
  expect(() => $props.splitByGroups({ a: 1 }, ["a"])).toThrow("requires the reze compiler");
  expect(() => $props.omit({ a: 1 }, "a")).toThrow("requires the reze compiler");
});
