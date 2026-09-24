import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile } from "@rezejs/compiler";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { cleanup, fire, mount, tick } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { hydrate, renderToString } from "../src";

afterEach(cleanup);

const scenario = `
import { signal } from "@rezejs/signals";
import { Show } from "@rezejs/dom/flow";
import { For } from "@rezejs/dom/list";

function Badge(props) {
  return <span class={["badge", { hot: props.count > 2 }]}>{props.label}: {props.count}</span>;
}

function Card(props) {
  return <section {...props.attrs}><h2>{props.title}</h2>{props.children}</section>;
}

export function setup() {
  const [count, setCount] = signal(1);
  const [items, setItems] = signal(["a", "b"]);
  const [name, setName] = signal("<Reze>");
  const App = () => (
    <main id="app">
      <h1 title={name()}>Hello {name()}!</h1>
      <Badge label="count" count={count()} />
      <Card title="list" attrs={{ "data-n": count() }}>
        <ul class={{ many: items().length > 2 }}>
          <For each={items()}>{(item) => <li>{item()}</li>}</For>
        </ul>
        <Show when={count() > 2} fallback={<p>small</p>}>
          <p>big {count()}</p>
        </Show>
      </Card>
      {count() % 2 === 0 ? <em>even</em> : "odd"}
      <>{name()}<b>!</b>{count()}</>
      <p>total: {count() * 3}</p>
      <p>{count() * 3}{-count()}</p>
      <p>name: {\`\${name()}\`}</p>
      <p>{\`\${count()}!\`}</p>
      <input value={name()} checked={count() > 1} />
      <button onClick={() => setCount(count() + 1)}>+</button>
    </main>
  );
  const steps = [
    () => setCount(2),
    () => setItems(["b", "c", "a"]),
    () => setName("x & y"),
    () => setCount(4),
  ];
  return { App, steps };
}
`;

interface Scenario {
  setup: () => { App: () => JSX.Element; steps: (() => void)[] };
}

const generated = join(import.meta.dirname, ".generated");

async function load(target: "client" | "server" | "hydrate"): Promise<Scenario> {
  const out = compile(scenario, "hydrate-scenario.tsx", {
    moduleName: "@rezejs/dom",
    sourceMap: false,
    target,
  });
  if (!out?.code) throw new Error(out?.diagnostics.map((d) => d.rendered).join("\n") ?? "no JSX");
  mkdirSync(generated, { recursive: true });
  const file = join(generated, `hydrate-scenario-${target}.ts`);
  writeFileSync(file, out.code);
  return import(/* @vite-ignore */ file);
}

/**
 * What the user sees: markup without what only hydration leaves behind (insert brackets,
 * hydration keys), and the input's live properties rather than the attributes the server
 * renders them as.
 */
function visible(el: Element): string {
  const input = el.querySelector("input")!;
  const markup = el.innerHTML
    .replace(/<!--[[\]]-->/g, "")
    .replace(/ data-hk="[^"]*"/g, "")
    .replace(/<input(?:[^>"]|"[^"]*")*>/, "<input>");
  return `${markup} value=${input.value} checked=${input.checked}`;
}

async function serverHTML(): Promise<string> {
  const server = await load("server");
  return renderToString(() => server.setup().App());
}

async function serverRendered(): Promise<HTMLElement> {
  const html = await serverHTML();
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.appendChild(container);
  return container;
}

test("the server renders what the client renders", async () => {
  const container = await serverRendered();
  const client = await load("client");
  const { el } = mount(() => client.setup().App());
  expect(visible(container)).toBe(visible(el));
});

test("server output escapes text and attribute values", async () => {
  const html = await serverHTML();
  expect(html).toContain('<h1 title="<Reze>">Hello <!--[-->&lt;Reze><!--]-->');
  expect(html).toContain('<input value="<Reze>">');
});

test("hydration adopts every server-rendered node and changes nothing", async () => {
  const container = await serverRendered();
  const markup = container.innerHTML;
  const nodes: Node[] = [];
  const walker = document.createTreeWalker(container);
  while (walker.nextNode()) nodes.push(walker.currentNode);

  const { App } = (await load("hydrate")).setup();
  hydrate(() => App(), container);

  expect(container.innerHTML).toBe(markup);
  const after = document.createTreeWalker(container);
  for (const node of nodes) {
    after.nextNode();
    expect(after.currentNode).toBe(node);
  }
});

test("hydrated DOM updates like a client render", async () => {
  const container = await serverRendered();
  const hydrated = (await load("hydrate")).setup();
  hydrate(() => hydrated.App(), container);
  const client = (await load("client")).setup();
  const { el } = mount(() => client.App());

  expect(visible(container)).toBe(visible(el));
  for (let i = 0; i < hydrated.steps.length; i++) {
    hydrated.steps[i]!();
    client.steps[i]!();
    tick();
    expect(visible(container)).toBe(visible(el));
  }

  fire(container.querySelector("button")!, "click");
  fire(el.querySelector("button")!, "click");
  tick();
  expect(container.querySelector("p")!.textContent).toBe("big 5");
  expect(visible(container)).toBe(visible(el));
});

test("templates without a server counterpart are created instead of claimed", async () => {
  const { App } = (await load("hydrate")).setup();
  const container = document.createElement("div");
  container.innerHTML = "<p>stale</p>";
  document.body.appendChild(container);
  hydrate(() => App(), container);

  const client = (await load("client")).setup();
  const { el } = mount(() => client.App());
  expect(visible(container)).toBe(visible(el));
});
