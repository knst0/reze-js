import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile } from "@rezejs/compiler";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { cleanup, mount, tick } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

afterEach(cleanup);

const scenario = `
import { signal } from "@rezejs/dom";

const [title] = signal("Reze");
const [label] = signal({ text: "static" });
const [count, setCount] = signal(0);
const DEBUG = false;

export const steps = [() => setCount(1), () => setCount(-2), () => setCount(3)];

export const App = () => (
  <section class={["box", { negative: count() < 0 }]}>
    <h1 title={title()}>{title()}</h1>
    <p>{label().text}: {count()}</p>
    {false && <b>never</b>}
    {true ? <i>{count() * 2}</i> : <u>never</u>}
    {DEBUG && <pre>debug</pre>}
    {count() > 2 ? <strong>big</strong> : null}
  </section>
);
`;

const generated = join(import.meta.dirname, ".generated");

interface Scenario {
  App: () => JSX.Element;
  steps: (() => void)[];
}

async function load(optimize: boolean): Promise<{ code: string; module: Scenario }> {
  const out = compile(scenario, "scenario.tsx", {
    moduleName: "@rezejs/dom",
    optimize,
    sourceMap: false,
  });
  if (!out?.code) throw new Error(out?.diagnostics.map((d) => d.rendered).join("\n") ?? "no JSX");
  mkdirSync(generated, { recursive: true });
  const file = join(generated, `scenario-${optimize ? "optimized" : "plain"}.ts`);
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

test("optimized and plain output render the same DOM at every step", async () => {
  const optimized = await load(true);
  const plain = await load(false);
  expect(optimized.code).toContain('const title = "Reze"');
  expect(optimized.code).not.toContain("never");
  expect(plain.code).toContain('signal("Reze")');
  expect(trace(optimized.module)).toEqual(trace(plain.module));
});
