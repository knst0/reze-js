import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { defaultClientConditions, defaultServerConditions, resolveConfig } from "vite";
import { afterEach, beforeEach, expect, test } from "vite-plus/test";

import reze from "../src";
import { HtmlEnv } from "../src/server/adapter";
import { buildSsgFixture } from "./ssg-harness";

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "reze-conditions-"));
  const modules = join(root, "node_modules");
  mkdirSync(join(modules, "@fixture", "label"), { recursive: true });
  const shared = join(import.meta.dirname, "..", "node_modules");
  for (const name of readdirSync(shared)) symlinkSync(join(shared, name), join(modules, name), "dir");
  writeFileSync(
    join(modules, "@fixture", "label", "package.json"),
    JSON.stringify({ name: "@fixture/label", type: "module", exports: { ".": { reze: "./reze.js" } } }),
  );
  writeFileSync(join(modules, "@fixture", "label", "reze.js"), 'export const label = "from-reze";\n');
  mkdirSync(join(root, "src", "routes"), { recursive: true });
  writeFileSync(
    join(root, "src", "routes", "index.tsx"),
    'import { label } from "@fixture/label";\nexport default function App() { return <p>{label}</p>; }\n',
  );
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

test("both environments add the reze conditions on top of vite's defaults", async () => {
  const config = await resolveConfig(
    { root, configFile: false, logLevel: "silent", plugins: [reze({ fileRoutes: { types: false }, ssg: true })] },
    "build",
  );
  expect(config.environments.client!.resolve.conditions).toEqual(["reze", ...defaultClientConditions]);
  expect(config.environments[HtmlEnv]!.resolve.conditions).toEqual(["reze-html", "reze", ...defaultServerConditions]);
});

test("a package that exports only the reze condition renders on the server without extra config", async () => {
  const outDir = join(root, "dist");
  await buildSsgFixture({ fixtureDir: root, outDir, plugins: reze({ fileRoutes: { types: false }, ssg: true }) });
  expect(readFileSync(join(outDir, "index.html"), "utf8")).toContain("from-reze");
}, 30_000);
