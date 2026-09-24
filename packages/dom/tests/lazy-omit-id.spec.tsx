import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { compile } from "@rezejs/compiler";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { flushSync, signal } from "@rezejs/signals";
import { cleanup, mount } from "@rezejs/test-utils";
import { afterEach, expect, test } from "vitest";

import { createUniqueId, hydrate, lazy, omit, renderToString, spread } from "../src";
import { Errored } from "../src/flow";
import { Loading } from "../src/loading";

afterEach(cleanup);

async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
  flushSync();
}

test("lazy shows the Loading fallback until the module arrives, then renders with props", async () => {
  const gate = Promise.withResolvers<{ default: (props: { name: string }) => JSX.Element }>();
  let loads = 0;
  const Profile = lazy(() => {
    loads++;
    return gate.promise;
  });
  const [name, setName] = signal("ada");
  const { el } = mount(() => (
    <Loading fallback={<i>loading</i>}>
      <Profile name={name()} />
    </Loading>
  ));
  expect(el.innerHTML).toBe("<i>loading</i>");
  gate.resolve({ default: (props) => <b>{props.name}</b> });
  await settle();
  expect(el.innerHTML).toBe("<b>ada</b>");
  setName("grace");
  flushSync();
  expect(el.innerHTML).toBe("<b>grace</b>");
  const second = mount(() => <Profile name="sync" />);
  expect(second.el.innerHTML).toBe("<b>sync</b>");
  expect(loads).toBe(1);
});

test("lazy picks a named export, and preload loads ahead of rendering", async () => {
  const About = lazy(() => Promise.resolve({ About: () => <p>about</p>, other: 1 }), {
    export: "About",
  });
  await About.preload();
  const { el } = mount(() => <About />);
  expect(el.innerHTML).toBe("<p>about</p>");
});

test("a failed load reaches Errored", async () => {
  const Broken = lazy(() => Promise.reject<{ default: () => JSX.Element }>(new Error("offline")));
  const { el } = mount(() => (
    <Errored fallback={(error) => <p>{(error() as Error).message}</p>}>
      <Broken />
    </Errored>
  ));
  await settle();
  expect(el.innerHTML).toBe("<p>offline</p>");
});

test("omit hides keys and keeps the rest live", () => {
  const [title, setTitle] = signal("a");
  const props = {
    get title() {
      return title();
    },
    label: "x",
    value: 1,
  };
  const rest = omit(props, "label", "value");
  expect(Object.keys(rest)).toEqual(["title"]);
  expect("label" in rest).toBe(false);
  expect((rest as Record<string, unknown>).label).toBeUndefined();
  setTitle("b");
  expect(rest.title).toBe("b");
  const byRule = omit({ $a: 1, b: 2 }, (key) => String(key).startsWith("$"));
  expect({ ...byRule }).toEqual({ b: 2 });
});

test("spreading an omit view sets only the visible keys", () => {
  const el = document.createElement("div");
  spread(el, omit({ title: "t", secret: "s", id: "i" }, "secret"), false, true);
  expect(el.getAttribute("title")).toBe("t");
  expect(el.hasAttribute("secret")).toBe(false);
  expect(el.id).toBe("i");
});

test("createUniqueId differs per call in a client render", () => {
  const ids = [createUniqueId(), createUniqueId()];
  expect(ids[0]).not.toBe(ids[1]);
});

const scenario = `
import { createUniqueId } from "@rezejs/dom";

function Field(props) {
  const id = createUniqueId();
  return (
    <p>
      <label for={id}>{props.label}</label>
      <input id={id} />
    </p>
  );
}

export const App = () => (
  <form>
    <Field label="a" />
    <Field label="b" />
  </form>
);
`;

async function load(target: "server" | "hydrate") {
  const out = compile(scenario, "id-scenario.tsx", {
    moduleName: "@rezejs/dom",
    sourceMap: false,
    target,
  });
  if (!out?.code) throw new Error(out?.diagnostics.map((d) => d.rendered).join("\n") ?? "no JSX");
  const dir = join(import.meta.dirname, ".generated");
  mkdirSync(dir, { recursive: true });
  const file = join(dir, `id-scenario-${target}.ts`);
  writeFileSync(file, out.code);
  return import(/* @vite-ignore */ file);
}

test("createUniqueId gives the server's ids back when hydrating", async () => {
  const server = await load("server");
  const html = renderToString(() => server.App());
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.appendChild(container);
  const serverIds = [...container.querySelectorAll("input")].map((input) => input.id);
  expect(new Set(serverIds).size).toBe(2);
  const client = await load("hydrate");
  hydrate(() => client.App(), container);
  const labels = [...container.querySelectorAll("label")].map((label) => label.htmlFor);
  expect(labels).toEqual(serverIds);
  expect([...container.querySelectorAll("input")].map((input) => input.id)).toEqual(serverIds);
});
