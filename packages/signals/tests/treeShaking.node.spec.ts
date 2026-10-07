import { fileURLToPath } from "node:url";

import { rolldown } from "rolldown";
import { expect, test } from "vitest";

const entry = fileURLToPath(new URL("../src/index.ts", import.meta.url));
const profile = fileURLToPath(new URL("../src/profile.ts", import.meta.url));
const nodeKinds = [
  "SignalNode",
  "ComputedNode",
  "EffectNode",
  "EffectScopeNode",
  "RenderNode",
  "SelectorKeyNode",
  "AsyncComputedNode",
  "BoundaryNode",
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
  ["computed", ["ComputedNode"]],
  ["effect", ["EffectNode"]],
  ["effectScope", ["EffectScopeNode"]],
  ["trigger", []],
  ["store", ["SignalNode"]],
  ["readonly", ["SignalNode"]],
  ["action", ["SignalNode"]],
  ["selector", ["SignalNode", "RenderNode", "SelectorKeyNode"]],
  ["asyncComputed", ["SignalNode", "AsyncComputedNode"]],
  ["boundary", ["SignalNode", "BoundaryNode"]],
])("bundling { %s } keeps only %j", async (imports, expected) => {
  expect(await bundledNodeKinds(imports)).toEqual(expected);
});

test("production bundles drop dev-only cycle detection", async () => {
  const imports = "signal, computed, effect";
  expect(await bundle(imports, "development")).toMatch(/Cycle detected/);
  const production = await bundle(imports, "production");
  expect(production).not.toMatch(/Cycle detected|isOnCheckPath|process\.env/);
});

test("a store without `action` carries only the empty write hook, not the journal", async () => {
  expect(await bundle("store")).not.toMatch(/journalWrite|ActionRun|layers/);
  expect(await bundle("store, action")).toMatch(/journalWrite/);
});

test("production bundles drop the dev-only write and call checks", async () => {
  const imports = "store, computed, action";
  expect(await bundle(imports, "development")).toMatch(/pureNodes/);
  expect(await bundle(imports, "production")).not.toMatch(/pureNodes|isPureRun|markPure|render binding/);
});

test("production bundles drop the profile channel even when a session can install it", async () => {
  const imports = "signal, computed, effect, root, asyncComputed";
  const installable = `export { setProfileHook } from ${JSON.stringify(profile)};`;
  expect(await bundle(imports, "development", installable)).toMatch(/profileHook/);
  expect(await bundle(imports, "production", installable)).not.toMatch(
    /profileHook\.|profileCreated|profileReran|profileWrote|profileDisposed|profileComponent|startProfileSession|"rerun"/,
  );
});

test("production bundles drop the fixed render binding's change check", async () => {
  const render = fileURLToPath(new URL("../src/render.ts", import.meta.url));
  const fixed = `export { fixedRenderEffect } from ${JSON.stringify(render)};`;
  expect(await bundle("signal", "development", fixed)).toMatch(/A compiled binding read/);
  expect(await bundle("signal", "production", fixed)).not.toMatch(/A compiled binding read|rerunChecked|warnedFixed/);
});
