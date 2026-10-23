import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { build, resolveConfig } from "vite";
import { afterEach, beforeEach, expect, test } from "vite-plus/test";

import reze from "../src";
import { BeforeLoadFlag } from "../src/before-load";

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "reze-before-load-"));
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(join(root, "src", "main.ts"), 'import { config } from "@fixture/routes";\nconsole.log(config);\n');
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

async function flagFor(command: "build" | "serve"): Promise<unknown> {
  const config = await resolveConfig({ root, configFile: false, logLevel: "silent", plugins: [reze()] }, command);
  return config.define?.[BeforeLoadFlag];
}

test("a build compiles the beforeLoad cascade out until a source mentions beforeLoad", async () => {
  expect(await flagFor("build")).toBe("false");
  expect(await flagFor("serve")).toBeUndefined();
  writeFileSync(join(root, "src", "account.ts"), "export const route = { beforeLoad: () => ({ user: 1 }) };\n");
  expect(await flagFor("build")).toBeUndefined();
});

test("a dependency that defines beforeLoad fails a build that compiled the cascade out", async () => {
  const pkg = join(root, "node_modules", "@fixture", "routes");
  mkdirSync(pkg, { recursive: true });
  writeFileSync(join(pkg, "package.json"), JSON.stringify({ name: "@fixture/routes", type: "module", exports: "./index.js" }));
  writeFileSync(join(pkg, "index.js"), "export const config = { beforeLoad: () => ({ user: 1 }) };\n");
  await expect(
    build({
      root,
      configFile: false,
      logLevel: "silent",
      plugins: [reze()],
      build: { write: false, rollupOptions: { input: join(root, "src", "main.ts") } },
    }),
  ).rejects.toThrow(/@fixture\/routes\/index\.js uses beforeLoad.*declare the beforeLoad route config in a module under/);
}, 30_000);
