import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile } from "@rezejs/compiler";
import { cleanup, mount, tick } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import type { JSX } from "../src/jsx";

afterEach(cleanup);

const source = `
import { For } from "@rezejs/dom/list";
import { store } from "@rezejs/signals";

const [state, setState] = store({ todos: [{ text: "a", done: false }, { text: "b", done: false }] });
export function List() {
  return (
    <div>
      <span>{state.todos.length}</span>
      <ul>
        <For each={state.todos}>{(item) => <li>{item().text}</li>}</For>
      </ul>
    </div>
  );
}

export const push = (text) => setState((d) => { d.todos.push({ text, done: false }); });
export const pop = () => setState((d) => { d.todos.pop(); });
export const splice = () => setState((d) => { d.todos.splice(1, 1); });
export const reverse = () => setState((d) => { d.todos.reverse(); });
export const rename = (i, text) => setState((d) => { d.todos[i].text = text; });
export const toggle = (i) => setState((d) => { d.todos[i].done = !d.todos[i].done; });
`;

const generated = join(import.meta.dirname, ".generated", "store-arrays");

interface Scenario {
  List: () => JSX.Element;
  push: (text: string) => void;
  pop: () => void;
  splice: () => void;
  reverse: () => void;
  rename: (i: number, text: string) => void;
  toggle: (i: number) => void;
}
async function load(optimize: boolean, tag = ""): Promise<Scenario> {
  const out = compile(source, "store-arrays.tsx", {
    moduleName: "@rezejs/dom",
    sourceMap: false,
    optimize,
    target: "client",
  });
  if (!out?.code) throw new Error(out?.diagnostics.map((d) => d.rendered).join("\n") ?? "no JSX");
  mkdirSync(generated, { recursive: true });
  const file = join(generated, `scenario-${optimize ? "opt" : "proxy"}${tag}.ts`);
  writeFileSync(file, out.code);
  return import(/* @vite-ignore */ file) as Promise<Scenario>;
}

function attach(List: () => JSX.Element): HTMLElement {
  return mount(List, "div").el;
}
function texts(el: Element): string[] {
  return [...el.querySelectorAll("li")].map((li) => li.textContent ?? "");
}

test("unproxied array stores render like the Proxy store after every step", async () => {
  const unproxied = await load(true);
  const proxy = await load(false);
  const fast = attach(unproxied.List);
  const slow = attach(proxy.List);
  const snapshot = (): [string[], string[], string, string] => [
    texts(fast),
    texts(slow),
    fast.querySelector("span")!.textContent ?? "",
    slow.querySelector("span")!.textContent ?? "",
  ];
  expect(snapshot()[0]).toEqual(snapshot()[1]);

  unproxied.push("c");
  proxy.push("c");
  tick();
  expect(snapshot()[0]).toEqual(["a", "b", "c"]);
  expect(snapshot()[0]).toEqual(snapshot()[1]);
  expect(snapshot()[2]).toBe("3");
  expect(snapshot()[2]).toEqual(snapshot()[3]);

  unproxied.rename(0, "z");
  proxy.rename(0, "z");
  tick();
  expect(snapshot()[0]).toEqual(snapshot()[1]);

  unproxied.toggle(1);
  proxy.toggle(1);
  tick();
  expect(snapshot()[0]).toEqual(snapshot()[1]);

  unproxied.reverse();
  proxy.reverse();
  tick();
  expect(snapshot()[0]).toEqual(snapshot()[1]);

  unproxied.splice();
  proxy.splice();
  tick();
  expect(snapshot()[0]).toEqual(snapshot()[1]);
  expect(snapshot()[2]).toEqual(snapshot()[3]);

  unproxied.pop();
  proxy.pop();
  tick();
  expect(snapshot()[0]).toEqual(snapshot()[1]);
});

test("keyed rows keep their DOM nodes across unproxied updates", async () => {
  const unproxied = await load(true, "-keyed");
  const el = attach(unproxied.List);
  const [a, b] = [...el.querySelectorAll("li")];
  unproxied.push("c");
  tick();
  expect([...el.querySelectorAll("li")].slice(0, 2)).toEqual([a, b]);
  unproxied.splice();
  tick();
  const rows = [...el.querySelectorAll("li")];
  expect(rows[0]).toBe(a);
  expect(texts(el)).toEqual(["a", "c"]);
});
