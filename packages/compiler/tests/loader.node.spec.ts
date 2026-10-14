import { expect, test } from "vite-plus/test";

import { compile } from "../index.js";

test("compile compiles a <p>hi</p> fixture", () => {
  const result = compile("<p>hi</p>", "fixture.tsx", { target: "client", moduleId: "fixture.tsx", sourceMap: false });
  expect(typeof result.code).toBe("string");
  expect(result.code!.length).toBeGreaterThan(0);
  expect(result.code).toContain("hi");
  expect(Array.isArray(result.diagnostics)).toBe(true);
});
