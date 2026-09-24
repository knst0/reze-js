import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile, link, summarize } from "@rezejs/compiler";
import { cleanup, fire, tick } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { hydrate, hydrateIslands, renderToString } from "../src";
import type { JSX } from "../src/jsx";

afterEach(cleanup);

const card = `
import { signal } from "@rezejs/signals";

export function Card(props) {
  const [open, setOpen] = signal(false);
  return (
    <section>
      <h2>{props.title}</h2>
      <button onClick={() => setOpen(!open())}>{props.children}</button>
      <footer>{props.footer}</footer>
    </section>
  );
}
`;

const page = (inner: string): string => `
import { Card } from "./card";

export function Page(props) {
  return <main>${inner}</main>;
}
`;

const files: Record<string, string> = {
  "/card.tsx": card,
  "/page.tsx": page("<Card title={props.title} footer={<i>note</i>}><b>body</b></Card>"),
  "/doubled.tsx": `
import { signal } from "@rezejs/signals";

export function Doubled(props) {
  const [n, setN] = signal(0);
  return <div onClick={() => setN(n() + 1)}>{props.children}{props.children}</div>;
}
`,
  "/twice.tsx": page("<Doubled><b>x</b></Doubled>").replace(
    'import { Card } from "./card";',
    'import { Doubled } from "./doubled";',
  ),
};

const generated = join(import.meta.dirname, ".generated", "islands-slots");

async function build(
  entries: string[],
  target: "server" | "hydrate" | "client",
  islands: boolean,
): Promise<Record<string, Record<string, never>>> {
  const summaries = Object.fromEntries(
    entries.map((id) => {
      const out = summarize(files[id]!, id, { moduleName: "@rezejs/dom" });
      if (!out.summary) throw new Error(out.diagnostics.map((d) => d.rendered).join("\n"));
      return [id, out];
    }),
  );
  const modules = entries.map((id) => ({
    id,
    summary: summaries[id]!.summary!,
    resolved: (summaries[id]!.specifiers as string[]).map((specifier) =>
      specifier.startsWith(".") ? specifier.replace("./", "/").replace(/(\.tsx?)?$/, ".tsx") : null,
    ),
    isEntry: id === "/page.tsx" || id === "/twice.tsx",
  }));
  const linked = link(modules, { optimize: true, islands, root: "/" });
  const dir = join(
    generated,
    `${entries.join("+").replace(/\//g, "")}.${target}.${islands ? "islands" : "plain"}`,
  );
  mkdirSync(dir, { recursive: true });
  const loaded: Record<string, Record<string, never>> = {};
  for (const id of entries) {
    const out = compile(files[id]!, id, {
      moduleName: "@rezejs/dom",
      sourceMap: false,
      target,
      facts: (linked.facts as Record<string, string>)[id],
    });
    if (!out?.code) throw new Error(out?.diagnostics.map((d) => d.rendered).join("\n") ?? "no JSX");
    const file = join(dir, `${id.replace(/\//g, "")}`);
    writeFileSync(file, out.code);
    loaded[id] = (await import(/* @vite-ignore */ file)) as Record<string, never>;
  }
  return loaded;
}

function islandId(html: string): string {
  return html.match(/<!--\$([^:]+):/)![1]!;
}

function attach(html: string): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.appendChild(container);
  return container;
}

function domText(el: Element): string {
  return (el.textContent ?? "").replace(/\s+/g, " ").trim();
}

test("slots render as marked ranges and hydrate into the island's inserts", async () => {
  const server = await build(["/card.tsx", "/page.tsx"], "server", true);
  const html = renderToString(
    () => (server["/page.tsx"]!.Page as (props: unknown) => JSX.Element)({ title: "Docs" }),
    true,
  ) as string;
  expect(html).toContain("<!--$slot:children-->");
  expect(html).toContain("<!--$slot:footer-->");
  expect(html).toMatch(/<!--\$[^:]+:[^:]+:\{[^}]*\}:(eager|lazy)-->/);

  const client = await build(["/card.tsx", "/page.tsx"], "hydrate", true);
  const el = attach(html);
  const before = domText(el);
  const dispose = hydrateIslands(el, { [islandId(html)]: client["/card.tsx"]!.Card });
  expect(domText(el)).toBe(before);
  expect(el.querySelector("h2")!.textContent).toBe("Docs");
  expect(el.querySelector("footer")!.textContent).toBe("note");
  expect(el.querySelector("button")!.textContent).toBe("body");

  fire(el.querySelector("button")!, "click");
  tick();
  expect(el.querySelector("button")!.textContent).toBe("body");
  dispose();
});

test("a hydrated slot page matches full hydration after every step", async () => {
  const server = await build(["/card.tsx", "/page.tsx"], "server", true);
  const html = renderToString(
    () => (server["/page.tsx"]!.Page as (props: unknown) => JSX.Element)({ title: "Docs" }),
    true,
  ) as string;
  const islands = await build(["/card.tsx", "/page.tsx"], "hydrate", true);
  const islandEl = attach(html);
  const islandDispose = hydrateIslands(islandEl, { [islandId(html)]: islands["/card.tsx"]!.Card });

  const full = await build(["/card.tsx", "/page.tsx"], "hydrate", false);
  const fullEl = attach(html);
  const fullDispose = hydrate(
    () => (full["/page.tsx"]!.Page as (props: unknown) => JSX.Element)({ title: "Docs" }),
    fullEl,
  );
  const snapshot = (): [string, string] => [domText(islandEl), domText(fullEl)];
  expect(snapshot()[0]).toBe(snapshot()[1]);
  for (const el of [islandEl, fullEl]) {
    fire(el.querySelector("button")!, "click");
    tick();
  }
  expect(snapshot()[0]).toBe(snapshot()[1]);
  islandDispose();
  fullDispose();
});

test("inserting one slot twice moves it, like the same value twice", async () => {
  const server = await build(["/doubled.tsx", "/twice.tsx"], "server", true);
  const html = renderToString(
    () => (server["/twice.tsx"]!.Page as (props: unknown) => JSX.Element)({ title: "t" }),
    true,
  ) as string;
  expect(html.match(/<!--\$slot:children-->/g)).toHaveLength(2);

  const client = await build(["/doubled.tsx", "/twice.tsx"], "hydrate", true);
  const el = attach(html);
  hydrateIslands(el, { [islandId(html)]: client["/doubled.tsx"]!.Doubled });
  const div = el.querySelector("div")!;
  expect(div.children).toHaveLength(1);
  expect(div.children[0]!.tagName).toBe("B");
});
