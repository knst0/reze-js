import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile } from "@rezejs/compiler";
import { flushSync, signal } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { createContext, hydrate, renderToString, useContext } from "../src";
import { Show } from "../src/flow";
import { For } from "../src/list";

afterEach(cleanup);

test("a provider reaches components, For rows and Show branches below it", () => {
  const Theme = createContext("light");
  const Label = () => <b>{useContext(Theme)}</b>;
  const [rows, setRows] = signal(["a"]);
  const [open, setOpen] = signal(false);
  const { el } = mount(() => (
    <Theme value="dark">
      <Label />
      <For each={rows()}>{() => <Label />}</For>
      <Show when={open()}>
        <Label />
      </Show>
    </Theme>
  ));
  setRows(["a", "b"]);
  setOpen(true);
  flushSync();
  expect(el.textContent).toBe("darkdarkdarkdark");
});

test("without a provider the default applies, and inner providers win", () => {
  const Theme = createContext("light");
  const Label = () => <i>{useContext(Theme)}</i>;
  const { el } = mount(() => (
    <>
      <Label />
      <Theme value="dark">
        <Label />
        <Theme value="blue">
          <Label />
        </Theme>
      </Theme>
    </>
  ));
  expect(el.textContent).toBe("lightdarkblue");
});

test("a getter value shares reactive state", () => {
  const [count, setCount] = signal(1);
  const Count = createContext(() => 0);
  const Show = () => <b>{useContext(Count)()}</b>;
  const { el } = mount(() => (
    <Count value={count}>
      <Show />
    </Count>
  ));
  setCount(2);
  flushSync();
  expect(el.textContent).toBe("2");
});

const scenario = `
import { createContext, useContext } from "@rezejs/dom";

const Theme = createContext("light");

function Label() {
  return <b>{useContext(Theme)}</b>;
}

export const App = () => (
  <main>
    <Theme value="dark">
      <Label />
    </Theme>
  </main>
);
`;

async function load(target: "server" | "hydrate") {
  const out = compile(scenario, "context-scenario.tsx", {
    moduleName: "@rezejs/dom",
    sourceMap: false,
    target,
  });
  if (!out?.code) throw new Error(out?.diagnostics.map((d) => d.rendered).join("\n") ?? "no JSX");
  const dir = join(import.meta.dirname, ".generated");
  mkdirSync(dir, { recursive: true });
  const file = join(dir, `context-scenario-${target}.ts`);
  writeFileSync(file, out.code);
  return import(/* @vite-ignore */ file);
}

test("the server renders provided values and hydration keeps them", async () => {
  const server = await load("server");
  const html = renderToString(() => server.App());
  expect(html).toContain(">dark</b>");
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.appendChild(container);
  const label = container.querySelector("b")!;
  const client = await load("hydrate");
  hydrate(() => client.App(), container);
  expect(container.querySelector("b")).toBe(label);
  expect(label.textContent).toBe("dark");
});
