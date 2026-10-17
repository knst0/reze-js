import { mergeStreamedHead } from "@rezejs/dom/internal/client";
import { signal } from "@rezejs/signals";
import { cleanup, mount, tick } from "@rezejs/testing-library";
import { Head, Show } from "reze-js";
import { afterEach, beforeEach, expect, test } from "vite-plus/test";

let headSnapshot = "";

beforeEach(() => {
  headSnapshot = document.head.innerHTML;
  const settings = (window as unknown as { happyDOM?: { settings?: Record<string, unknown> } }).happyDOM?.settings;
  if (settings !== undefined) settings["handleDisabledFileLoadingAsSuccess"] = true;
});

afterEach(() => {
  cleanup();
  document.head.innerHTML = headSnapshot;
});

function managed(): Element[] {
  return [...document.head.querySelectorAll("[data-rz-head]")];
}

function tag(kind: string, key: string, value: string): Element | undefined {
  return managed().find((element) => element.tagName === kind && element.getAttribute(key) === value);
}

function streamUnit(key: string, html: string): HTMLTemplateElement {
  const unit = document.createElement("template");
  unit.setAttribute("data-rz-head", "");
  unit.setAttribute("data-rz-head-key", key);
  unit.innerHTML = html;
  return unit;
}

test("Head mounts tags into the head with no inline trace and dispose removes them", async () => {
  const { el, dispose } = mount(() => (
    <p>
      <Head>
        <script src="/sdk.js" async />
        <meta name="theme-color" content="black" />
      </Head>
    </p>
  ));
  expect(el.innerHTML).toBe("<p></p>");
  expect(tag("SCRIPT", "src", "/sdk.js")?.hasAttribute("async")).toBe(true);
  expect(tag("META", "name", "theme-color")?.getAttribute("content")).toBe("black");
  expect(managed()).toHaveLength(2);

  dispose();
  expect(managed()).toHaveLength(1);
  await Promise.resolve();
  expect(managed()).toHaveLength(0);
});

test("a script adopted from the server keeps one tag across owners until all dispose", () => {
  const server = document.createElement("script");
  server.setAttribute("src", "/sdk.js");
  server.setAttribute("data-rz-head", "");
  document.head.append(server);

  const first = mount(() => (
    <Head>
      <script src="/sdk.js" />
    </Head>
  ));
  const second = mount(() => (
    <Head>
      <script src="/sdk.js" />
    </Head>
  ));
  expect(tag("SCRIPT", "src", "/sdk.js")).toBe(server);

  first.dispose();
  tick();
  expect(tag("SCRIPT", "src", "/sdk.js")).toBe(server);

  second.dispose();
  tick();
  expect(tag("SCRIPT", "src", "/sdk.js")).toBe(server);
});

test("a meta shadows the page tag so it stays live, and disposing shows the page tag again", () => {
  const [theme, setTheme] = signal("white");
  const server = document.createElement("meta");
  server.setAttribute("name", "theme-color");
  server.setAttribute("content", "black");
  server.setAttribute("data-rz-head", "");
  document.head.append(server);

  const { dispose } = mount(() => (
    <Head>
      <meta name="theme-color" content={theme()} />
    </Head>
  ));
  const live = tag("META", "name", "theme-color");
  expect(live).not.toBe(server);
  expect(live?.getAttribute("content")).toBe("white");

  setTheme("red");
  tick();
  expect(tag("META", "name", "theme-color")?.getAttribute("content")).toBe("red");

  dispose();
  tick();
  expect(tag("META", "name", "theme-color")).toBe(server);
  expect(document.head.querySelectorAll('meta[name="theme-color"]')).toHaveLength(1);
});

test("an unkeyed script adopts on matching content and is created otherwise", () => {
  const server = document.createElement("script");
  server.setAttribute("type", "application/ld+json");
  server.setAttribute("data-rz-head", "u0");
  server.textContent = '{"a":1}';
  document.head.append(server);

  const first = mount(() => (
    <Head>
      <script type="application/ld+json">
        {"{"}"a":1{"}"}
      </script>
    </Head>
  ));
  expect(tag("SCRIPT", "type", "application/ld+json")).toBe(server);

  const second = mount(() => (
    <Head>
      <script type="application/ld+json">
        {"{"}"b":2{"}"}
      </script>
    </Head>
  ));
  expect(managed().filter((element) => element.tagName === "SCRIPT")).toHaveLength(2);

  first.dispose();
  second.dispose();
  tick();
  expect(tag("SCRIPT", "type", "application/ld+json")).toBe(server);
});

test("mergeStreamedHead appends, replaces by key and replaces unkeyed tags by stream token", () => {
  mergeStreamedHead(document, streamUnit("meta:name:theme", `<meta name="theme" content="dark" data-rz-head="">`));
  const first = tag("META", "name", "theme");
  expect(first?.getAttribute("content")).toBe("dark");

  mergeStreamedHead(document, streamUnit("meta:name:theme", `<meta name="theme" content="light" data-rz-head="">`));
  const replaced = tag("META", "name", "theme");
  expect(replaced).not.toBe(first);
  expect(replaced?.getAttribute("content")).toBe("light");

  mergeStreamedHead(document, streamUnit("meta:name:theme", ""));
  expect(tag("META", "name", "theme")).toBeUndefined();
  {
    mergeStreamedHead(document, streamUnit("u0", `<script type="application/ld+json" data-rz-head="u0">{"a":1}</script>`));
    const first = tag("SCRIPT", "type", "application/ld+json");
    expect(first?.textContent).toBe('{"a":1}');

    mergeStreamedHead(document, streamUnit("u0", `<script type="application/ld+json" data-rz-head="u0">{"a":2}</script>`));
    expect(tag("SCRIPT", "type", "application/ld+json")).not.toBe(first);
    expect(managed().filter((element) => element.tagName === "SCRIPT")).toHaveLength(1);
  }
});

test("title and style mount from Head and a later owner shadows an earlier one per key", () => {
  document.title = "Document";
  const template = document.head.querySelector("title");
  const [page, setPage] = signal("One");
  const outer = mount(() => (
    <Head>
      <title>Site</title>
      <style>{"body{margin:0}"}</style>
    </Head>
  ));
  expect(document.title).toBe("Site");
  expect(managed().map((element) => element.tagName)).toEqual(["TITLE", "STYLE"]);

  const inner = mount(() => (
    <Head>
      <title>{page()} | Site</title>
    </Head>
  ));
  expect(document.title).toBe("One | Site");
  setPage("Two");
  tick();
  expect(document.title).toBe("Two | Site");
  expect(document.head.querySelectorAll("title")).toHaveLength(1);

  inner.dispose();
  tick();
  expect(document.title).toBe("Site");

  outer.dispose();
  tick();
  expect(document.head.querySelector("title")).toBe(template);
  expect(document.title).toBe("Document");
  expect(managed()).toHaveLength(0);
  document.title = "";
});

test("a streamed unit for a claimed key becomes the tag shown after the owner disposes", () => {
  const { dispose } = mount(() => (
    <Head>
      <meta name="theme" content="client" />
    </Head>
  ));
  mergeStreamedHead(document, streamUnit("meta:name:theme", `<meta name="theme" content="server" data-rz-head="">`));
  expect(tag("META", "name", "theme")?.getAttribute("content")).toBe("client");

  dispose();
  tick();
  expect(tag("META", "name", "theme")?.getAttribute("content")).toBe("server");
  expect(document.head.querySelectorAll('meta[name="theme"]')).toHaveLength(1);
});
