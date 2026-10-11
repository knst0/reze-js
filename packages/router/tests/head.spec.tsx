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

function setup(routes: RouteDefinition[], initial: string, headDefaults?: { title: string }): (to: string) => void {
  const history = createMemoryHistory(initial);
  const Router = createRouter({ routes, history, headDefaults });
  let navigate!: Navigate;
  function Root(props: { children: JSX.Element }) {
    navigate = useNavigate();
    return <>{props.children}</>;
  }
  mount(() => <Router root={Root} />);
  return (to: string) => navigate(to as never);
}

const Home = () => null;

test("commits insert marked head tags and leaving removes them back to the template baseline", () => {
  document.title = "Base";
  const templateDescription = document.createElement("meta");
  templateDescription.setAttribute("name", "description");
  templateDescription.setAttribute("content", "base desc");
  document.head.append(templateDescription);
  const templateScript = document.createElement("script");
  templateScript.setAttribute("src", "/template.js");
  document.head.append(templateScript);

  const navigate = setup(
    [
      { path: "/", component: Home },
      {
        path: "/about",
        component: Home,
        meta: {
          title: "About",
          description: "about desc",
          scripts: [
            { src: "/widget.js", async: true },
            { src: "/ordered.js", defer: true, type: "text/javascript", crossorigin: "anonymous", integrity: "sha1-x" },
          ],
          links: [{ rel: "preconnect", href: "https://cdn.example.test" }],
          metas: [{ property: "og:title", content: "About" }],
        },
      },
    ],
    "/",
  );
  expect(document.title).toBe("Base");
  expect(managed()).toHaveLength(0);

  navigate("/about");
  tick();
  expect(document.title).toBe("About");
  expect(document.head.querySelector('meta[name="description"]:not([data-rz-head])')?.getAttribute("content")).toBe("about desc");
  const widget = tag("SCRIPT", "src", "/widget.js");
  expect(widget?.hasAttribute("async")).toBe(true);
  expect((widget as HTMLScriptElement | undefined)?.async).toBe(true);
  const ordered = tag("SCRIPT", "src", "/ordered.js") as HTMLScriptElement | undefined;
  expect(ordered?.hasAttribute("defer")).toBe(true);
  expect(ordered?.getAttribute("integrity")).toBe("sha1-x");
  expect(ordered?.async).toBe(false);
  expect(tag("LINK", "href", "https://cdn.example.test")?.getAttribute("rel")).toBe("preconnect");
  expect(tag("META", "property", "og:title")?.getAttribute("content")).toBe("About");
  expect(managed()).toHaveLength(4);
  expect(document.head.contains(templateScript)).toBe(true);

  navigate("/");
  tick();
  expect(document.title).toBe("Base");
  expect(document.head.querySelector('meta[name="description"]')?.getAttribute("content")).toBe("base desc");
  expect(managed()).toHaveLength(0);
  expect(document.head.contains(templateScript)).toBe(true);
});

test("a script shared by two routes keeps its node while changed attributes replace it", () => {
  const navigate = setup(
    [
      { path: "/a", component: Home, meta: { scripts: [{ src: "/shared.js" }] } },
      { path: "/b", component: Home, meta: { scripts: [{ src: "/shared.js" }] } },
      { path: "/c", component: Home, meta: { scripts: [{ src: "/shared.js", async: true }] } },
    ],
    "/a",
  );
  const first = tag("SCRIPT", "src", "/shared.js");
  expect(first).not.toBeUndefined();

  navigate("/b");
  tick();
  expect(tag("SCRIPT", "src", "/shared.js")).toBe(first);

  navigate("/c");
  tick();
  const replaced = tag("SCRIPT", "src", "/shared.js");
  expect(replaced).not.toBeUndefined();
  expect(replaced).not.toBe(first);
  expect(replaced?.hasAttribute("async")).toBe(true);
});

test("a managed entry suppresses the same-key scalar without duplicating the tag", () => {
  const baseline = document.createElement("meta");
  baseline.setAttribute("name", "description");
  baseline.setAttribute("content", "base desc");
  document.head.append(baseline);

  const navigate = setup(
    [
      { path: "/", component: Home },
      {
        path: "/about",
        component: Home,
        meta: {
          description: "scalar desc",
          canonical: "https://example.test/scalar",
          links: [{ rel: "canonical", href: "https://example.test/managed" }],
          metas: [{ name: "description", content: "managed desc" }],
        },
      },
    ],
    "/",
  );
  navigate("/about");
  tick();
  const descriptions = [...document.head.querySelectorAll('meta[name="description"]')];
  expect(descriptions).toHaveLength(1);
  expect(descriptions[0]?.hasAttribute("data-rz-head")).toBe(true);
  expect(descriptions[0]?.getAttribute("content")).toBe("managed desc");
  const canonicals = [...document.head.querySelectorAll('link[rel="canonical"]')];
  expect(canonicals).toHaveLength(1);
  expect(canonicals[0]?.getAttribute("href")).toBe("https://example.test/managed");

  navigate("/");
  tick();
  expect(document.head.querySelector('meta[name="description"]')?.getAttribute("content")).toBe("base desc");
  expect(document.head.querySelector('link[rel="canonical"]')).toBeNull();
  expect(managed()).toHaveLength(0);
});

test("headDefaults restore scalar fields the route does not set", () => {
  setup([{ path: "/", component: Home }], "/", { title: "Studio" });
  expect(document.title).toBe("Studio");
});

test("a Head adopts the route script and the tag leaves with the route", () => {
  const About = () => (
    <Head>
      <script src="/shared.js" />
    </Head>
  );
  const navigate = setup(
    [
      { path: "/", component: Home },
      { path: "/about", component: About, meta: { scripts: [{ src: "/shared.js" }] } },
    ],
    "/",
  );
  navigate("/about");
  tick();
  const adopted = tag("SCRIPT", "src", "/shared.js");
  expect(adopted).not.toBeUndefined();
  expect(document.head.querySelectorAll('script[src="/shared.js"]')).toHaveLength(1);

  navigate("/");
  tick();
  expect(tag("SCRIPT", "src", "/shared.js")).toBeUndefined();
  expect(adopted?.isConnected).toBe(false);
});

test("a Head meta replaces the route meta and the route claim survives the Head dispose", () => {
  const [open, setOpen] = signal(true);
  const About = () => (
    <Show when={open()}>
      <Head>
        <meta property="og:title" content="head" />
      </Head>
    </Show>
  );
  setup([{ path: "/about", component: About, meta: { metas: [{ property: "og:title", content: "route" }] } }], "/about");
  const live = tag("META", "property", "og:title");
  expect(document.head.querySelectorAll('meta[property="og:title"]')).toHaveLength(1);
  expect(live?.getAttribute("content")).toBe("head");

  setOpen(false);
  tick();
  expect(tag("META", "property", "og:title")).toBe(live);
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
