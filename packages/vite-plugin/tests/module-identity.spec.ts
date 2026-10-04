import { expect, test } from "vitest";

import { canonicalModuleId, compilerInputHash, createModuleRegistry } from "../src/module-identity";

test.each([
  ["/proj/src/App.tsx", "/proj/", "src/App.tsx"],
  ["./src/App.tsx", "/proj", "src/App.tsx"],
  ["/src/App.tsx", "/", "src/App.tsx"],
  ["C:\\proj\\src\\App.tsx", "C:/proj", "src/App.tsx"],
  ["/proj/src/App.tsx?t=1736123456789", "/proj", "src/App.tsx"],
  ["/proj/src/App.tsx?raw&t=1736123456789&v=revision", "/proj", "src/App.tsx?raw&v=revision"],
  ["/proj/src/App.tsx?url&t=1", "/proj", "src/App.tsx?url&t=1"],
  ["/proj/src/App.tsx?vue&type=script&lang.ts", "/proj", "src/App.tsx?vue&type=script&lang.ts"],
  ["/proj/src/App.tsx?%74=1736123456789", "/proj", "src/App.tsx?%74=1736123456789"],
  ["/proj/src/App.tsx?t=1736123456789#fragment", "/proj", "src/App.tsx#fragment"],
  ["/proj/node_modules/ui/index.ts", "/proj", "node_modules/ui/index.ts"],
  ["/store/node_modules/ui/index.ts", "/proj", "ui/index.ts"],
  ["/store/node_modules/.pnpm/ui@1/node_modules/@scope/ui/index.ts", "/proj", "@scope/ui/index.ts"],
  ["\0reze:ssg-view", "/proj", "virtual:reze:ssg-view"],
  ["\0virtual:reze-routes", "/proj", "virtual:reze-routes"],
  ["virtual:reze-routes?raw&t=1736123456789", "/proj", "virtual:reze-routes?raw"],
])("canonicalizes %s under %s without stripping semantic identity", (id, root, expected) => {
  expect(canonicalModuleId(id, root)).toBe(expected);
});

test("external linked packages use their own package-relative identity", () => {
  expect(canonicalModuleId("/one/shared/src/ui.tsx?raw", "/one/app", {
    packageName: "@acme/ui", packageRoot: "/one/shared",
  })).toBe("@acme/ui/src/ui.tsx?raw");
  expect(canonicalModuleId("/two/shared/src/ui.tsx?raw", "/two/app", {
    packageName: "@acme/ui", packageRoot: "/two/shared",
  })).toBe("@acme/ui/src/ui.tsx?raw");
  expect(() => canonicalModuleId("/unknown/ui.tsx", "/proj")).toThrow("owning package");
  expect(() => canonicalModuleId("/outside/ui.tsx", "/proj", {
    packageName: "@acme/ui", packageRoot: "/other",
  })).toThrow("owning package");
});

test("compiler site preimage matches the cross-language BLAKE3 contract", () => {
  expect(compilerInputHash("src/App.tsx", "export default 1;")).toBe("617ae233d41b19b1");
});

test("paired target registration permits identical input but rejects differing module source", () => {
  const registry = createModuleRegistry();
  expect(registry.register("src/App.tsx", "export default 1;")).toBe("617ae233d41b19b1");
  expect(registry.register("src/App.tsx", "export default 1;")).toBe("617ae233d41b19b1");
  expect(registry.ids()).toEqual(["src/App.tsx"]);
  expect(() => registry.register("src/App.tsx", "export default 2;")).toThrow("different compiler input across targets");
  expect(createModuleRegistry().register("src/App.tsx", "export default 2;")).not.toBe("617ae233d41b19b1");
});
