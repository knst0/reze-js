import { fileURLToPath } from "node:url";

import { rolldown } from "rolldown";
import { expect, test } from "vitest";

const entry = fileURLToPath(new URL("../src/index.ts", import.meta.url));
const devtools = fileURLToPath(new URL("../src/devtools.ts", import.meta.url));
const nodeKinds = [
  "SignalNode",
  "LayeredSignalNode",
  "ComputedNode",
  "EffectNode",
  "EffectScopeNode",
  "RenderNode",
  "SelectorKeyNode",
  "AsyncComputedNode",
];

async function bundle(imports: string, nodeEnv = "development", extraSource = ""): Promise<string> {
  const build = await rolldown({
    input: "entry",
    plugins: [
      {
        name: "entry",
        resolveId: (id) => (id === "entry" ? id : undefined),
        load: (id) => (id === "entry" ? `export { ${imports} } from ${JSON.stringify(entry)};${extraSource}` : undefined),
      },
    ],
    transform: { define: { "process.env.NODE_ENV": JSON.stringify(nodeEnv) } },
  });
  const { output } = await build.generate({ format: "esm", minify: "dce-only" });
  await build.close();
  return output[0].code;
}

async function bundledNodeKinds(imports: string): Promise<string[]> {
  const code = await bundle(imports);
  return nodeKinds.filter((kind) => new RegExp(`\\b${kind}\\b`).test(code));
}

test.each([
  ["signal", ["SignalNode"]],
  ["layeredSignal", ["SignalNode", "LayeredSignalNode"]],
  ["computed", ["ComputedNode"]],
  ["effect", ["EffectNode"]],
  ["effectScope", ["EffectScopeNode"]],
  ["trigger", []],
  ["store", ["SignalNode"]],
  ["selector", ["SignalNode", "RenderNode", "SelectorKeyNode"]],
  ["asyncComputed", ["SignalNode", "AsyncComputedNode"]],
])("bundling { %s } keeps only %j", async (imports, expected) => {
  expect(await bundledNodeKinds(imports)).toEqual(expected);
});

test("production bundles drop dev-only cycle detection", async () => {
  const imports = "signal, computed, effect";
  expect(await bundle(imports, "development")).toMatch(/Cycle detected/);
  const production = await bundle(imports, "production");
  expect(production).not.toMatch(/Cycle detected|isOnCheckPath|process\.env/);
});

test("production bundles drop the devtools hook even when devtools can install it", async () => {
  const imports = "signal, computed, effect, root, asyncComputed";
  const installable = `export { setDebugHook } from ${JSON.stringify(devtools)};`;
  expect(await bundle(imports, "development", installable)).toMatch(/debugHook/);
  expect(await bundle(imports, "production", installable)).not.toMatch(/debugHook\.|rerunning|written/);
});
