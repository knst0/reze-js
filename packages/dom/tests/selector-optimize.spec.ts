import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile } from "@rezejs/compiler";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { cleanup, mount, tick } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

afterEach(cleanup);

const scenario = `
import { signal } from "@rezejs/dom";
import { For } from "@rezejs/dom/list";

const [rows, setRows] = signal([{ id: 1 }, { id: 2 }, { id: 3 }]);
const [selected, setSelected] = signal(0);

export const steps = [
  () => setSelected(2),
  () => setSelected(3),
  () => setRows([{ id: 3 }, { id: 4 }, { id: 1 }]),
  () => setSelected(4),
  () => { setRows([{ id: 5 }]); setSelected(5); },
  () => setSelected(0),
];

export const App = () => (
  <ul>
    <For each={rows()} key={(row) => row.id}>
      {(row) => (
        <li class={selected() === row().id ? "on" : ""} title={row().id !== selected() ? "off" : "on"}>
          {row().id}
          {selected() === row().id && <b>*</b>}
        </li>
      )}
    </For>
  </ul>
);
`;

const generated = join(import.meta.dirname, ".generated");

interface Scenario {
  App: () => JSX.Element;
  steps: (() => void)[];
}

async function load(optimize: boolean): Promise<{ code: string; module: Scenario }> {
  const out = compile(scenario, "selector-scenario.tsx", {
    moduleName: "@rezejs/dom",
    optimize,
    sourceMap: false,
  });
  if (!out?.code) throw new Error(out?.diagnostics.map((d) => d.rendered).join("\n") ?? "no JSX");
  mkdirSync(generated, { recursive: true });
  const file = join(generated, `selector-scenario-${optimize ? "optimized" : "plain"}.ts`);
  writeFileSync(file, out.code);
  return { code: out.code, module: await import(/* @vite-ignore */ file) };
}

function trace(module: Scenario): string[] {
  const { el } = mount(module.App);
  const snapshots = [el.innerHTML];
  for (const step of module.steps) {
    step();
    tick();
    snapshots.push(el.innerHTML);
  }
  return snapshots;
}

test("rows reading a selector render the same DOM as plain comparisons at every step", async () => {
  const optimized = await load(true);
  const plain = await load(false);
  expect(optimized.code).toContain("selector(selected)");
  expect(plain.code).not.toContain("selector(");
  const optimizedTrace = trace(optimized.module);
  expect(optimizedTrace[1]).toContain('<li class="on" title="on">2<b>*</b></li>');
  expect(optimizedTrace).toEqual(trace(plain.module));
});
