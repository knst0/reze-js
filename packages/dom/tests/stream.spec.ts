import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile } from "@rezejs/compiler";
import { cleanup, tick } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { applyStreamChunks, hydrate, render, renderToStream } from "../src";
import type { JSX } from "../src/jsx";
afterEach(cleanup);
const source = `
export let resolveUser = (u) => {};
export let resolveSide = (t) => {};

export async function User() {
  const user = await new Promise((r) => { resolveUser = r; });
  return <ul data-name={user.name}><li>user</li></ul>;
}

export async function Side(props) {
  const text = await new Promise((r) => { resolveSide = r; });
  return <b>{text}</b>;
}

export function Page() {
  return (
    <main>
      <User />
      <Side text="side" />
    </main>
  );
}

export function Loader() {
  return <User />;
}
`;

const generated = join(import.meta.dirname, ".generated", "stream");

interface Scenario {
  Page: () => JSX.Element;
  Loader: () => JSX.Element;
  resolveUser: (u: unknown) => void;
  resolveSide: (t: unknown) => void;
}

async function build(target: "server" | "hydrate" | "client"): Promise<Scenario> {
  const out = compile(source, "stream-scenario.tsx", {
    moduleName: "@rezejs/dom",
    sourceMap: false,
    target,
  });
  if (!out?.code) throw new Error(out?.diagnostics.map((d) => d.rendered).join("\n") ?? "no JSX");
  const dir = join(generated, target);
  mkdirSync(dir, { recursive: true });
  const file = join(dir, "scenario.ts");
  writeFileSync(file, out.code);
  return import(/* @vite-ignore */ file) as Promise<Scenario>;
}

async function collect(stream: ReadableStream<Uint8Array>): Promise<string> {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let html = "";
  for (;;) {
    const chunk: ReadableStreamReadResult<Uint8Array> = await reader.read();
    if (chunk.done) break;
    html += decoder.decode(chunk.value, { stream: true });
  }
  html += decoder.decode();
  return html;
}

function attach(html: string): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.appendChild(container);
  return container;
}

test("the shell streams at once and chunks resolve in any order", async () => {
  const server = await build("server");
  const stream = renderToStream(() => server.Page());
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  const first = await reader.read();
  const shell: string = decoder.decode(first.value);
  expect(shell).toContain("<!--$s:");
  expect(shell).not.toContain("<li>user</li>");
  expect(shell).not.toContain("<b>side</b>");

  server.resolveSide("side");
  const second = decoder.decode((await reader.read()).value);
  expect(second).toContain("data-reze-chunk");
  expect(second).toContain("side</b>");

  server.resolveUser({ name: "Ada" });
  const rest = await collect(
    new ReadableStream<Uint8Array>({
      start(controller) {
        const pump = (): void => {
          void reader.read().then((chunk: ReadableStreamReadResult<Uint8Array>) => {
            if (chunk.done) controller.close();
            else {
              controller.enqueue(chunk.value);
              pump();
            }
          });
        };
        pump();
      },
    }),
  );
  const html = shell + second + rest;
  expect(html).toContain("<li>user</li>");
  expect(html).toContain('data-name="Ada"');
  const applied = attach(html);
  applyStreamChunks(applied);
  expect(applied.querySelector("main")!.textContent).toContain("user");
  expect(applied.querySelector("main")!.textContent).toContain("side");
});

test("a collected stream matches the client DOM", async () => {
  const server = await build("server");
  const stream = renderToStream(() => server.Page());
  const collected = collect(stream);
  server.resolveSide("side");
  server.resolveUser({ name: "Ada" });
  const html = await collected;
  expect(html).toContain("<li>user</li>");

  const client = await build("client");
  const el = attach("<div></div>");
  const dispose = render(() => client.Page(), el);
  client.resolveSide("side");
  client.resolveUser({ name: "Ada" });
  await Promise.resolve();
  tick();
  expect(el.querySelector("main")!.textContent).toBe(
    (() => {
      const applied = attach(html);
      applyStreamChunks(applied);
      return applied.querySelector("main")!.textContent;
    })(),
  );
  dispose();
});

test("a timed-out stream leaves the fallbacks", async () => {
  const server = await build("server");
  const html = await collect(renderToStream(() => server.Loader(), { timeoutMs: 20 }));
  expect(html).toContain("<!--$s:");
  expect(html).not.toContain("<li>user</li>");
});

test("unserializable stream values error the stream", async () => {
  const server = await build("server");
  const stream = renderToStream(() => server.Loader());
  const reader = stream.getReader();
  await reader.read();
  server.resolveUser(() => ({}));
  await expect(reader.read()).rejects.toThrow("[STREAM_VALUES]");
});

test("hydration adopts streamed chunks without calling the loader", async () => {
  const server = await build("server");
  const stream = renderToStream(() => server.Loader());
  const collected = collect(stream);
  server.resolveUser({ name: "Ada" });
  const html = await collected;
  expect(html).toContain("data-reze-chunk");

  const client = await build("hydrate");
  const before = client.resolveUser;
  const el = attach(html);
  const dispose = hydrate(() => client.Loader(), el);
  await Promise.resolve();
  tick();
  expect(el.querySelector("ul")!.getAttribute("data-name")).toBe("Ada");
  expect(el.querySelector("li")!.textContent).toBe("user");
  expect(client.resolveUser).toBe(before);
  dispose();
});
