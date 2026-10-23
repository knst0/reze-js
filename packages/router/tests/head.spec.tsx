import { signal } from "@rezejs/signals";
import { cleanup, mount, tick } from "@rezejs/testing-library";
import { Head, Show, type JSX } from "reze-js";
import { afterEach, beforeEach, expect, test } from "vite-plus/test";

import { createMemoryHistory, createRouter, useNavigate, type Navigate, type RouteDefinition } from "../src";
import { syncManagedHead } from "../src/head";

let headSnapshot = "";

beforeEach(() => {
  headSnapshot = document.head.innerHTML;
  const settings = (window as unknown as { happyDOM?: { settings?: Record<string, unknown> } }).happyDOM?.settings;
  if (settings !== undefined) settings["handleDisabledFileLoadingAsSuccess"] = true;
});

afterEach(() => {
  cleanup();
  document.head.innerHTML = headSnapshot;
  window.history.replaceState(null, "", "/");
});

function managed(): Element[] {
  return [...document.head.querySelectorAll("[data-rz-head]")];
}

function tag(kind: string, key: string, value: string): Element | undefined {
  return managed().find((element) => element.tagName === kind && element.getAttribute(key) === value);
}

function setup(routes: RouteDefinition[], initial: string): (to: string) => void {
  const history = createMemoryHistory(initial);
  const Router = createRouter({ routes, history });
  let navigate!: Navigate;
  function Root(props: { children: JSX.Element }) {
    navigate = useNavigate();
    return <>{props.children}</>;
  }
  mount(() => <Router root={Root} />);
  return (to: string) => navigate(to as never);
}

const Home = () => null;

test("layout and leaf Head tags shadow the document head with the deepest winning, and leaving shows each shadowed tag again", () => {
  document.title = "Base";
  const templateTitle = document.head.querySelector("title")!;
  const templateDescription = document.createElement("meta");
  templateDescription.setAttribute("name", "description");
  templateDescription.setAttribute("content", "base desc");
  document.head.append(templateDescription);
  const templateCanonical = document.createElement("link");
  templateCanonical.setAttribute("rel", "canonical");
  templateCanonical.setAttribute("href", "https://site.example/");
  document.head.append(templateCanonical);
  const templateScript = document.createElement("script");
  templateScript.setAttribute("src", "/template.js");
  document.head.append(templateScript);

  const Docs = (props: { children: JSX.Element }) => (
    <section>
      <Head>
        <title>Docs</title>
        <meta name="description" content="docs desc" />
        <link rel="preconnect" href="https://cdn.example.test" />
      </Head>
      {props.children}
    </section>
  );
  const Leaf = () => (
    <Head>
      <title>Leaf</title>
      <link rel="canonical" href="https://site.example/docs/leaf" />
      <meta property="og:title" content="Leaf" />
    </Head>
  );
  const navigate = setup(
    [
      { path: "/", component: Home },
      {
        path: "/docs",
        component: Docs,
        children: [
          { path: "/", component: Home },
          { path: "/leaf", component: Leaf },
        ],
      },
    ],
    "/",
  );
  expect(document.title).toBe("Base");
  expect(managed()).toHaveLength(0);

  navigate("/docs/leaf");
  tick();
  expect(document.title).toBe("Leaf");
  expect(document.head.querySelectorAll("title")).toHaveLength(1);
  expect([...document.head.querySelectorAll('meta[name="description"]')].map((tag) => tag.getAttribute("content"))).toEqual(["docs desc"]);
  expect([...document.head.querySelectorAll('link[rel="canonical"]')].map((tag) => tag.getAttribute("href"))).toEqual([
    "https://site.example/docs/leaf",
  ]);
  expect(tag("META", "property", "og:title")?.getAttribute("content")).toBe("Leaf");
  expect(tag("LINK", "rel", "preconnect")).not.toBeUndefined();

  navigate("/docs");
  tick();
  expect(document.title).toBe("Docs");
  expect(document.head.querySelector('link[rel="canonical"]')).toBe(templateCanonical);
  expect(tag("META", "property", "og:title")).toBeUndefined();

  navigate("/");
  tick();
  expect(document.title).toBe("Base");
  expect(document.head.querySelector("title")).toBe(templateTitle);
  expect(document.head.querySelector('meta[name="description"]')).toBe(templateDescription);
  expect(managed()).toHaveLength(0);
  expect(document.head.contains(templateScript)).toBe(true);
  document.title = "";
});

test("a script shared by two routes keeps its node and leaves after the turn no route claims it", async () => {
  const Shared = () => (
    <Head>
      <script src="/shared.js" />
    </Head>
  );
  const navigate = setup(
    [
      { path: "/a", component: Shared },
      { path: "/b", component: Shared },
      { path: "/c", component: Home },
    ],
    "/a",
  );
  const first = tag("SCRIPT", "src", "/shared.js");
  expect(first).not.toBeUndefined();

  navigate("/b");
  tick();
  expect(tag("SCRIPT", "src", "/shared.js")).toBe(first);
  expect(document.head.querySelectorAll('script[src="/shared.js"]')).toHaveLength(1);

  navigate("/c");
  tick();
  await Promise.resolve();
  expect(tag("SCRIPT", "src", "/shared.js")).toBeUndefined();
});

test("a Head inside a toggled view shadows the route's tag only while it is shown", () => {
  const [open, setOpen] = signal(true);
  const Page = () => (
    <>
      <Head>
        <meta property="og:title" content="route" />
      </Head>
      <Show when={open()}>
        <Head>
          <meta property="og:title" content="dialog" />
        </Head>
      </Show>
    </>
  );
  setup([{ path: "/", component: Page }], "/");
  expect(document.head.querySelectorAll('meta[property="og:title"]')).toHaveLength(1);
  expect(tag("META", "property", "og:title")?.getAttribute("content")).toBe("dialog");

  setOpen(false);
  tick();
  expect(document.head.querySelectorAll('meta[property="og:title"]')).toHaveLength(1);
  expect(tag("META", "property", "og:title")?.getAttribute("content")).toBe("route");

  setOpen(true);
  tick();
  expect(tag("META", "property", "og:title")?.getAttribute("content")).toBe("dialog");
});

test("syncManagedHead keeps shared tags, clones new scripts and adopts links and metas", () => {
  const templateScript = document.createElement("script");
  templateScript.setAttribute("src", "/template.js");
  document.head.append(templateScript);
  const shared = document.createElement("script");
  shared.setAttribute("src", "/shared.js");
  shared.setAttribute("data-rz-head", "");
  document.head.append(shared);
  const stale = document.createElement("link");
  stale.setAttribute("rel", "preconnect");
  stale.setAttribute("href", "https://old.example.test");
  stale.setAttribute("data-rz-head", "");
  document.head.append(stale);

  const incoming = new DOMParser().parseFromString(
    `<html><head><script src="/shared.js" data-rz-head></script>` +
      `<script src="/fresh.js" defer data-rz-head></script>` +
      `<link rel="preconnect" href="https://new.example.test" data-rz-head>` +
      `<meta property="og:title" content="New" data-rz-head></head><body></body></html>`,
    "text/html",
  );
  const incomingScript = incoming.head.querySelector('script[src="/fresh.js"]')!;
  const incomingLink = incoming.head.querySelector('link[href="https://new.example.test"]')!;
  const incomingMeta = incoming.head.querySelector('meta[property="og:title"]')!;

  syncManagedHead(document, incoming);

  expect(document.head.contains(templateScript)).toBe(true);
  expect(document.head.contains(stale)).toBe(false);
  expect(tag("SCRIPT", "src", "/shared.js")).toBe(shared);
  const fresh = tag("SCRIPT", "src", "/fresh.js") as HTMLScriptElement | undefined;
  expect(fresh).not.toBeUndefined();
  expect(fresh).not.toBe(incomingScript);
  expect(fresh?.hasAttribute("defer")).toBe(true);
  expect(fresh?.async).toBe(false);
  expect(tag("LINK", "href", "https://new.example.test")).toBe(incomingLink);
  expect(tag("META", "property", "og:title")).toBe(incomingMeta);
  expect(
    managed().map((element) => element.getAttribute("src") ?? element.getAttribute("href") ?? element.getAttribute("property")),
  ).toEqual(["/shared.js", "/fresh.js", "https://new.example.test", "og:title"]);
});

test("syncManagedHead keeps a claimed tag and shows the fetched one once its owner disposes", () => {
  const [open, setOpen] = signal(true);
  mount(() => (
    <Show when={open()}>
      <Head>
        <title>Island</title>
      </Head>
    </Show>
  ));
  const claimed = document.head.querySelector("title");
  expect(document.title).toBe("Island");

  const incoming = new DOMParser().parseFromString(
    `<html><head><title data-rz-head>Next page</title></head><body></body></html>`,
    "text/html",
  );
  syncManagedHead(document, incoming);
  expect(document.head.querySelector("title")).toBe(claimed);
  expect(document.title).toBe("Island");

  setOpen(false);
  tick();
  expect(document.title).toBe("Next page");
  expect(document.head.querySelectorAll("title")).toHaveLength(1);
});
