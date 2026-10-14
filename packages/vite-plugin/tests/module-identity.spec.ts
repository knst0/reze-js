import { expect, test } from "vite-plus/test";

import { canonicalModuleId, compilerInputHash, createModuleRegistry } from "../src/module-identity";

test("canonicalizes module ids under their root without stripping semantic identity", () => {
  const cases: [id: string, root: string, expected: string][] = [
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
  ];
  for (const [id, root, expected] of cases) {
    expect(canonicalModuleId(id, root), `${id} under ${root}`).toBe(expected);
  }
});

test("external linked packages use their own package-relative identity and unowned paths never fall back to cwd", () => {
  expect(
    canonicalModuleId("/one/shared/src/ui.tsx?raw", "/one/app", {
      packageName: "@acme/ui",
      packageRoot: "/one/shared",
    }),
  ).toBe("@acme/ui/src/ui.tsx?raw");
  expect(
    canonicalModuleId("/two/shared/src/ui.tsx?raw", "/two/app", {
      packageName: "@acme/ui",
      packageRoot: "/two/shared",
    }),
  ).toBe("@acme/ui/src/ui.tsx?raw");
  expect(() => canonicalModuleId("/unknown/ui.tsx", "/proj")).toThrow("owning package");
  expect(() =>
    canonicalModuleId("/outside/ui.tsx", "/proj", {
      packageName: "@acme/ui",
      packageRoot: "/other",
    }),
  ).toThrow("owning package");
  expect(() => canonicalModuleId("Q:/missing/source.tsx", "C:/app")).toThrow("owning package");
});

test("compiler site preimage matches the cross-language BLAKE3 contract", () => {
  expect(compilerInputHash("src/App.tsx", "export default 1;")).toBe("617ae233d41b19b1");
});

test("module registry accepts identical paired input, rejects differing source, and lets dev updates replace stored input", () => {
  const registry = createModuleRegistry();
  expect(registry.register("src/App.tsx", "export default 1;")).toBe("617ae233d41b19b1");
  expect(registry.register("src/App.tsx", "export default 1;")).toBe("617ae233d41b19b1");
  expect(() => registry.register("src/App.tsx", "export default 2;")).toThrow("different compiler input across targets");
  expect(createModuleRegistry().register("src/App.tsx", "export default 2;")).not.toBe("617ae233d41b19b1");

  const dev = createModuleRegistry();
  const first = dev.register("src/App.tsx", "export default 1;");
  expect(dev.update("src/App.tsx", "export default 1;")).toBe(first);
  const second = dev.update("src/App.tsx", "export default 2;");
  expect(second).not.toBe(first);
  expect(second).toBe(createModuleRegistry().register("src/App.tsx", "export default 2;"));
  expect(dev.register("src/App.tsx", "export default 2;")).toBe(second);
  expect(() => dev.register("src/App.tsx", "export default 1;")).toThrow("different compiler input across targets");
});
