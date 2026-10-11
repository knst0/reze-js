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

test("Head mounts tags into the head with no inline trace and dispose removes them", () => {
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
  expect(managed()).toHaveLength(0);
});

test("Head attributes and text stay live while mounted", () => {
  const [theme, setTheme] = signal("black");
  const [json, setJson] = signal('{"a":1}');
  mount(() => (
    <Head>
      <meta name="theme-color" content={theme()} />
      <script type="application/ld+json">{json()}</script>
    </Head>
  ));
  expect(tag("META", "name", "theme-color")?.getAttribute("content")).toBe("black");

  setTheme("white");
  setJson('{"a":2}');
  tick();
  expect(tag("META", "name", "theme-color")?.getAttribute("content")).toBe("white");
  expect(tag("SCRIPT", "type", "application/ld+json")?.textContent).toBe('{"a":2}');
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

test("a client-created script is shared while owners live and removed after the last dispose", () => {
  const first = mount(() => (
    <Head>
      <script src="/sdk.js" />
    </Head>
  ));
  const created = tag("SCRIPT", "src", "/sdk.js");
  const second = mount(() => (
    <Head>
      <script src="/sdk.js" />
    </Head>
  ));
  expect(tag("SCRIPT", "src", "/sdk.js")).toBe(created);

  first.dispose();
  tick();
  expect(tag("SCRIPT", "src", "/sdk.js")).toBe(created);

  second.dispose();
  tick();
  expect(tag("SCRIPT", "src", "/sdk.js")).toBeUndefined();
});

test("a meta replaces the adopted tag so it stays live", () => {
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
  expect(tag("META", "name", "theme-color")).toBeUndefined();
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

test("unmarked template tags are invisible: Head creates its own and never touches them", () => {
  const template = document.createElement("script");
  template.setAttribute("src", "/sdk.js");
  document.head.append(template);

  const { dispose } = mount(() => (
    <Head>
      <script src="/sdk.js" />
    </Head>
  ));
  const scripts = [...document.head.querySelectorAll('script[src="/sdk.js"]')];
  expect(scripts).toHaveLength(2);
  expect(document.head.contains(template)).toBe(true);

  dispose();
  tick();
  expect([...document.head.querySelectorAll('script[src="/sdk.js"]')]).toEqual([template]);
});

test("a conditional Head adds and removes its tags with the branch", () => {
  const [open, setOpen] = signal(true);
  mount(() => (
    <Show when={open()}>
      <Head>
        <meta name="theme-color" content="black" />
      </Head>
    </Show>
  ));
  expect(tag("META", "name", "theme-color")).not.toBeUndefined();

  setOpen(false);
  tick();
  expect(tag("META", "name", "theme-color")).toBeUndefined();

  setOpen(true);
  tick();
  expect(tag("META", "name", "theme-color")).not.toBeUndefined();
});

test("mergeStreamedHead appends, replaces by key and removes on empty units", () => {
  mergeStreamedHead(document, streamUnit("meta:name:theme", `<meta name="theme" content="dark" data-rz-head="">`));
  const first = tag("META", "name", "theme");
  expect(first?.getAttribute("content")).toBe("dark");

  mergeStreamedHead(document, streamUnit("meta:name:theme", `<meta name="theme" content="light" data-rz-head="">`));
  const replaced = tag("META", "name", "theme");
  expect(replaced).not.toBe(first);
  expect(replaced?.getAttribute("content")).toBe("light");

  mergeStreamedHead(document, streamUnit("meta:name:theme", ""));
  expect(tag("META", "name", "theme")).toBeUndefined();
});

test("mergeStreamedHead replaces unkeyed tags by stream token", () => {
  mergeStreamedHead(document, streamUnit("u0", `<script type="application/ld+json" data-rz-head="u0">{"a":1}</script>`));
  const first = tag("SCRIPT", "type", "application/ld+json");
  expect(first?.textContent).toBe('{"a":1}');

  mergeStreamedHead(document, streamUnit("u0", `<script type="application/ld+json" data-rz-head="u0">{"a":2}</script>`));
  expect(tag("SCRIPT", "type", "application/ld+json")).not.toBe(first);
  expect(managed().filter((element) => element.tagName === "SCRIPT")).toHaveLength(1);
});
