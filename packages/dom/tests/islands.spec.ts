import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile } from "@rezejs/compiler";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { cleanup, fire, tick } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { hydrate, hydrateIslands, renderToString, ssr, ssrChild, ssrIsland } from "../src";

afterEach(cleanup);

const scenario = (boundary: string): string => `
import { signal } from "@rezejs/signals";
import { ssrIsland } from "@rezejs/dom";

export function Counter(props) {
  const [count, setCount] = signal(props.start);
  return <button onClick={() => setCount(count() + 1)}>{props.label} {count()}</button>;
}

export function Page() {
  return (
    <main>
      <h1>Static</h1>
      ${boundary}
      <p>after</p>
    </main>
  );
}
`;

const serverBoundary = `{ssrIsland("c1", Counter, { start: 1, label: "<a>", note: "--> <!--" })}`;
const clientBoundary = `<Counter start={1} label="<a>" note="--> <!--" />`;

interface Scenario {
  Counter: (props: { start: number; label: string }) => JSX.Element;
  Page: () => JSX.Element;
}

const generated = join(import.meta.dirname, ".generated");

async function load(target: "server" | "hydrate"): Promise<Scenario> {
  const source = scenario(target === "server" ? serverBoundary : clientBoundary);
  const out = compile(source, "islands-scenario.tsx", {
    moduleName: "@rezejs/dom",
    sourceMap: false,
    target,
  });
  if (!out?.code) throw new Error(out?.diagnostics.map((d) => d.rendered).join("\n") ?? "no JSX");
  mkdirSync(generated, { recursive: true });
  const file = join(generated, `islands-scenario-${target}.ts`);
  writeFileSync(file, out.code);
  return import(/* @vite-ignore */ file);
}

function withoutIslandMarkers(html: string): string {
  return html.replace(/<!--\$[^]*?-->|<!--\/\$-->/g, "");
}

function attach(html: string): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.appendChild(container);
  return container;
}

function nodesOf(container: Node): Node[] {
  const nodes: Node[] = [];
  const walker = document.createTreeWalker(container);
  while (walker.nextNode()) nodes.push(walker.currentNode);
  return nodes;
}

test("an island renders between markers naming its id, key scope and escaped JSON props", async () => {
  const { Page } = await load("server");
  expect(renderToString(() => Page(), true)).toBe(
    '<main data-hk="0"><h1>Static</h1><!--[-->' +
      '<!--$c1:1-:{"start":1,"label":"\\u003ca\\u003e","note":"\\u002d\\u002d\\u003e \\u003c!\\u002d\\u002d"}:eager-->' +
      '<button data-hk="1-0"><!--[-->&lt;a><!--]--> <!--[-->1<!--]--></button>' +
      "<!--/$--><!--]--><p>after</p></main>",
  );
});

test("island markers are the only difference from rendering without islands", async () => {
  const { Page } = await load("server");
  const withIslands = renderToString(() => Page(), true);
  expect(withIslands).not.toBe(renderToString(() => Page()));
  expect(withoutIslandMarkers(withIslands)).toBe(renderToString(() => Page()));
});

test("an island inside an island renders like a component, without markers", () => {
  const Inner = (props: { text: string }): JSX.Element =>
    ssr(["<i>", "</i>"], ssrChild(props.text));
  const Outer = (): JSX.Element =>
    ssr(["<b>", "</b>"], ssrChild(ssrIsland("inner", Inner, { text: "x" })));
  expect(renderToString(() => ssrIsland("outer", Outer, {}), true)).toBe(
    "<!--$outer:0-:{}:eager--><b><i>x</i></b><!--/$-->",
  );
});

test("island props that are not JSON fail the render with the island id and the key", () => {
  const Island = (): JSX.Element => null;
  const render = (props: object) => () =>
    renderToString(() => ssrIsland("x1", Island, props), true);
  expect(render({ onClick: () => {} })).toThrow(
    /^\[ISLAND_PROPS\] island "x1": props\.onClick is a function/,
  );
  expect(render({ list: [1, -0] })).toThrow(
    /^\[ISLAND_PROPS\] island "x1": props\.list\[1\] is -0/,
  );
  const cyclic: Record<string, unknown> = {};
  cyclic.self = cyclic;
  expect(render({ data: cyclic })).toThrow(
    /^\[ISLAND_PROPS\] island "x1": props\.data\.self is a cycle/,
  );
  expect(render({ when: new Date(0) })).toThrow(/props\.when/);
});

test("props reach the hydrated island as the values the server serialized", () => {
  const props = { n: -1.5, text: "<!-- a -- b -->", nested: { list: [true, null, "x"] } };
  const Island = (): JSX.Element => null;
  const container = attach(renderToString(() => ssrIsland("probe", Island, props), true));
  let received: unknown;
  hydrateIslands(container, {
    probe: (p) => {
      received = p;
      return null;
    },
  });
  expect(received).toEqual(props);
});

test("hydrating islands adopts every server-rendered node and changes nothing", async () => {
  const { Page } = await load("server");
  const container = attach(renderToString(() => Page(), true));
  const markup = container.innerHTML;
  const nodes = nodesOf(container);

  const { Counter } = await load("hydrate");
  hydrateIslands(container, { c1: Counter });

  expect(container.innerHTML).toBe(markup);
  const after = nodesOf(container);
  expect(after.length).toBe(nodes.length);
  after.forEach((node, i) => expect(node).toBe(nodes[i]));
});

test("a hydrated island updates like the same component under full hydration", async () => {
  const { Page: ServerPage } = await load("server");
  const { Counter, Page } = await load("hydrate");
  const islands = attach(renderToString(() => ServerPage(), true));
  const full = attach(renderToString(() => ServerPage()));
  hydrateIslands(islands, { c1: Counter });
  hydrate(() => Page(), full);

  for (let i = 0; i < 2; i++) {
    fire(islands.querySelector("button")!, "click");
    fire(full.querySelector("button")!, "click");
    tick();
    expect(withoutIslandMarkers(islands.innerHTML)).toBe(full.innerHTML);
  }
  expect(islands.querySelector("button")!.textContent).toBe("<a> 3");
});

test("the disposer stops island updates", async () => {
  const { Page } = await load("server");
  const { Counter } = await load("hydrate");
  const container = attach(renderToString(() => Page(), true));
  const dispose = hydrateIslands(container, { c1: Counter });
  const button = container.querySelector("button")!;
  fire(button, "click");
  tick();
  expect(button.textContent).toBe("<a> 2");
  dispose();
  fire(button, "click");
  tick();
  expect(button.textContent).toBe("<a> 2");
});

test("an island id missing from the component map throws before hydrating", async () => {
  const { Page } = await load("server");
  const { Counter } = await load("hydrate");
  const html = renderToString(() => Page(), true);
  const container = attach(
    html + renderToString(() => ssrIsland("other", Counter, { start: 0, label: "" }), true),
  );
  expect(() => hydrateIslands(container, { c1: Counter })).toThrow(/"other"/);
  fire(container.querySelector("button")!, "click");
  tick();
  expect(container.querySelector("button")!.textContent).toBe("<a> 1");
});
