import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile, link, summarize } from "@rezejs/compiler";
import { cleanup, fire, tick } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { hydrateIslands, renderToString } from "../src";
import type { JSX } from "../src/jsx";

afterEach(cleanup);
function sleep(ms: number): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, ms);
  return promise;
}
const files: Record<string, string> = {
  "/counter.tsx": `
import { signal } from "@rezejs/signals";

export function Eager() {
  const [count, setCount] = signal(1);
  return <button onClick={() => setCount(count() + 1)}>{count()}</button>;
}
export function Visible() {
  const [count, setCount] = signal(10);
  return <button onClick={() => setCount(count() + 1)}>{count()}</button>;
}
export function Idle() {
  const [count, setCount] = signal(100);
  return <button onClick={() => setCount(count() + 1)}>{count()}</button>;
}
export function Click() {
  const [count, setCount] = signal(1000);
  return <button onClick={() => setCount(count() + 1)}>{count()}</button>;
}
`,
  "/page.tsx": `
import { Click, Eager, Idle, Visible } from "./counter";

export function Page() {
  return (
    <main>
      <Eager />
      <Visible island:load="visible" />
      <Idle island:load="idle" />
      <Click island:load="interaction" />
    </main>
  );
}
`,
  "/client.tsx": `
import { hydrate } from "@rezejs/dom";
import { Page } from "./page";

export function hydratePage(el) {
  return hydrate(() => <Page />, el);
}
`,
};

const generated = join(import.meta.dirname, ".generated", "islands-lazy");

async function build(target: "server" | "hydrate"): Promise<Record<string, Record<string, never>>> {
  const entries = Object.keys(files);
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
      specifier.startsWith(".") ? `/${specifier.slice(2)}.tsx` : null,
    ),
    isEntry: id === "/client.tsx",
  }));
  const linked = link(modules, { optimize: true, islands: true, root: "/" });
  const dir = join(generated, target);
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

function attach(html: string): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.appendChild(container);
  return container;
}

interface ObserverInstance {
  target: Element | undefined;
  disconnect: () => void;
  fire: (isIntersecting: boolean) => void;
}

const observers: ObserverInstance[] = [];

function mockIntersectionObserver(): void {
  (window as unknown as Record<string, unknown>).IntersectionObserver = class {
    target: Element | undefined;
    constructor(private readonly callback: (entries: { isIntersecting: boolean }[]) => void) {
      observers.push(this as unknown as ObserverInstance);
    }
    observe(target: Element): void {
      this.target = target;
    }
    disconnect(): void {}
    fire(isIntersecting: boolean): void {
      this.callback([{ isIntersecting }]);
    }
  };
}

function buttons(el: Element): string[] {
  return [...el.querySelectorAll("button")].map((b) => b.textContent ?? "");
}

test("lazy markers carry the mode and hydrate on their trigger", async () => {
  mockIntersectionObserver();
  observers.length = 0;
  const server = await build("server");
  const html = renderToString(
    () => (server["/page.tsx"]!.Page as () => JSX.Element)(),
    true,
  ) as string;
  expect(html).toContain(":eager-->");
  expect(html).toContain(":visible-->");
  expect(html).toContain(":idle-->");
  expect(html).toContain(":interaction-->");

  const client = await build("hydrate");
  const el = attach(html);
  const dispose = (client["/client.tsx"]!.hydratePage as (el: Element) => () => void)(el);
  expect(buttons(el)).toEqual(["1", "10", "100", "1000"]);

  fire(el.querySelectorAll("button")[1]!, "click");
  tick();
  expect(buttons(el)[1]).toBe("10");

  const visible = observers.find((o) => o.target?.textContent === "10");
  expect(visible).toBeDefined();
  visible!.fire(false);
  visible!.fire(true);
  await sleep(20);
  tick();
  fire(el.querySelectorAll("button")[1]!, "click");
  tick();
  expect(buttons(el)[1]).toBe("11");
  dispose();
});

test("events that arrive before the load are lost", async () => {
  const server = await build("server");
  const html = renderToString(
    () => (server["/page.tsx"]!.Page as () => JSX.Element)(),
    true,
  ) as string;
  const client = await build("hydrate");
  const el = attach(html);
  const dispose = (client["/client.tsx"]!.hydratePage as (el: Element) => () => void)(el);

  const idle = el.querySelectorAll("button")[2]!;
  fire(idle, "click");
  tick();
  expect(idle.textContent).toBe("100");
  await sleep(20);
  tick();
  fire(idle, "click");
  tick();
  expect(idle.textContent).toBe("101");
  dispose();
});

test("an interaction inside the range loads the island at once", async () => {
  const server = await build("server");
  const html = renderToString(
    () => (server["/page.tsx"]!.Page as () => JSX.Element)(),
    true,
  ) as string;
  const client = await build("hydrate");
  const el = attach(html);
  const dispose = (client["/client.tsx"]!.hydratePage as (el: Element) => () => void)(el);

  const button = el.querySelectorAll("button")[3]!;
  fire(button, "pointerdown");
  await sleep(20);
  tick();
  fire(button, "click");
  tick();
  expect(button.textContent).toBe("1001");
  dispose();
});

test("the disposer cancels a pending lazy island", async () => {
  mockIntersectionObserver();
  observers.length = 0;
  const server = await build("server");
  const html = renderToString(
    () => (server["/page.tsx"]!.Page as () => JSX.Element)(),
    true,
  ) as string;
  const client = await build("hydrate");
  const el = attach(html);
  const dispose = (client["/client.tsx"]!.hydratePage as (el: Element) => () => void)(el);
  dispose();

  for (const observer of observers) observer.fire(true);
  await sleep(20);
  tick();
  expect(buttons(el)).toEqual(["1", "10", "100", "1000"]);
  expect(observers.length).toBeGreaterThan(0);
});

test("hydrateIslands accepts a descriptor directly", async () => {
  const server = await build("server");
  const html = renderToString(
    () => (server["/page.tsx"]!.Page as () => JSX.Element)(),
    true,
  ) as string;
  const ids = [...html.matchAll(/<!--\$([^:]+):/g)].map((match) => match[1]!);
  const client = await build("hydrate");
  const el = attach(html);
  const counter = client["/counter.tsx"]! as Record<string, never>;
  const dispose = hydrateIslands(
    el,
    Object.fromEntries(
      (["Eager", "Visible", "Idle", "Click"] as const).map((name, index) => [
        ids[index]!,
        { load: () => Promise.resolve(counter), mode: "eager", export: name },
      ]),
    ),
  );
  await sleep(20);
  fire(el.querySelectorAll("button")[0]!, "click");
  tick();
  expect(buttons(el)[0]).toBe("2");
  dispose();
});
