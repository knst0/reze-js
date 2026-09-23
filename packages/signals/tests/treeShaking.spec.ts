import { fileURLToPath } from "node:url";

import { build } from "esbuild";
import { expect, test } from "vitest";

const entry = fileURLToPath(new URL("../src/index.ts", import.meta.url));
const nodeKinds = ["SignalNode", "ComputedNode", "EffectNode", "EffectScopeNode"];

async function bundle(imports: string, nodeEnv = "development"): Promise<string> {
  const result = await build({
    stdin: {
      contents: `export { ${imports} } from ${JSON.stringify(entry)};`,
      resolveDir: process.cwd(),
      loader: "ts",
    },
    bundle: true,
    format: "esm",
    write: false,
    define: { "process.env.NODE_ENV": JSON.stringify(nodeEnv) },
    // Production builds minify; syntax minification is what folds the NODE_ENV branches.
    minifySyntax: true,
  });
  return result.outputFiles[0]!.text;
}

async function bundledNodeKinds(imports: string): Promise<string[]> {
  const code = await bundle(imports);
  return nodeKinds.filter((kind) => new RegExp(`\\b${kind}\\b`).test(code));
}

test.each([
  ["signal", ["SignalNode"]],
  ["computed", ["ComputedNode"]],
  ["effect", ["EffectNode"]],
  ["effectScope", ["EffectScopeNode"]],
  ["trigger", []],
  ["signal, computed", ["SignalNode", "ComputedNode"]],
])("bundling { %s } keeps only %j", async (imports, expected) => {
  expect(await bundledNodeKinds(imports)).toEqual(expected);
});

test("production bundles drop dev-only cycle detection", async () => {
  const imports = "signal, computed, effect";
  expect(await bundle(imports, "development")).toMatch(/Cycle detected/);
  const production = await bundle(imports, "production");
  expect(production).not.toMatch(/Cycle detected|isOnCheckPath|process\.env/);
});
