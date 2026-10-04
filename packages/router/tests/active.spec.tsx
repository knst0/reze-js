import { cleanup, mount, settle, tick } from "@rezejs/testing-library";
import { Show, type JSX } from "reze-js";
import { signal } from "@rezejs/signals";
import { afterEach, expect, test } from "vitest";

import {
  createBrowserHistory,
  createHashHistory,
  createMemoryHistory,
  createRouter,
  useLinkState,
  useNavigate,
  type LinkState,
  type Navigate,
  type RouteDefinition,
  type RouteModule,
  type RouterHistory,
} from "../src";

afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

const Page = () => <p>page</p>;
const CatchAll: RouteDefinition[] = [{ path: "/*rest", component: Page }];

function setup(nav: () => JSX.Element, initial: string, routes = CatchAll, history: RouterHistory = createMemoryHistory(initial)) {
  let navigate!: Navigate;
  function Root(props: { children: JSX.Element }) {
    navigate = useNavigate();
    return (
      <main>
        <nav>{nav()}</nav>
        {props.children}
      </main>
    );
  }
  const Router = createRouter({ routes, history });
  const { el } = mount(() => <Router root={Root} />);
  const anchor = (href: string): Element => el.querySelector(`a[href="${href}"]`)!;
  const go = (to: string): void => {
    navigate(to);
    tick();
  };
  return { el, anchor, go };
}

function stateOf(el: Element): string {
  return [
    el.getAttribute("aria-current") ?? "-",
    el.hasAttribute("data-active") ? "active" : "-",
    el.hasAttribute("data-pending") ? "pending" : "-",
  ].join(" ");
}

test("anchors carry current and active state for the current pathname", () => {
  const { anchor } = setup(
    () => (
      <>
        <a href="/">home</a>
        <a href="/blog">blog</a>
        <a href="/Blog/1/">post</a>
        <a href="/blogs">blogs</a>
        <a href="/blog/1?x=1#top">post query</a>
      </>
    ),
    "/blog/1",
  );
  expect(stateOf(anchor("/"))).toBe("- - -");
  expect(stateOf(anchor("/blog"))).toBe("- active -");
  expect(stateOf(anchor("/Blog/1/"))).toBe("page active -");
  expect(stateOf(anchor("/blogs"))).toBe("- - -");
  expect(stateOf(anchor("/blog/1?x=1#top"))).toBe("page active -");
});

test("navigation moves the state and anchors mounted later start correct", () => {
  const [isShown, setIsShown] = signal(false);
  const { anchor, go } = setup(
    () => (
      <>
        <a href="/">home</a>
        <a href="/about">about</a>
        <Show when={isShown()}>
          <a href="/about/team">team</a>
        </Show>
      </>
    ),
    "/",
  );
  expect(stateOf(anchor("/"))).toBe("page active -");
  go("/about/team");
  expect(stateOf(anchor("/"))).toBe("- - -");
  expect(stateOf(anchor("/about"))).toBe("- active -");
  setIsShown(true);
  tick();
  expect(stateOf(anchor("/about/team"))).toBe("page active -");
});

test("a dynamic href is bound, re-keys on change and is evaluated once per change", () => {
  const [to, setTo] = signal("/a");
  let evaluations = 0;
  const counted = (value: string): string => {
    evaluations++;
    return value;
  };
  const { el, go } = setup(() => <a href={counted(to())}>x</a>, "/a");
  const a = el.querySelector("nav a")!;
  expect(a.getAttribute("href")).toBe("/a");
  expect(stateOf(a)).toBe("page active -");
  expect(evaluations).toBe(1);
  go("/b");
  expect(stateOf(a)).toBe("- - -");
  setTo("/b");
  tick();
  expect(a.getAttribute("href")).toBe("/b");
  expect(stateOf(a)).toBe("page active -");
  expect(evaluations).toBe(2);
});

test("relative hrefs resolve against the current document URL and re-key on navigation", () => {
  window.history.replaceState(null, "", "/about/team");
  const { anchor, go } = setup(() => <a href="team">team</a>, "", CatchAll, createBrowserHistory());
  expect(stateOf(anchor("team"))).toBe("page active -");
  go("/about/other");
  expect(stateOf(anchor("team"))).toBe("- - -");
  go("/team");
  expect(stateOf(anchor("team"))).toBe("page active -");
});

test("hash history links written #/path carry state; in-page #anchors do not", () => {
  window.history.replaceState(null, "", "/#/about");
  const { anchor, go } = setup(
    () => (
      <>
        <a href="#/about">about</a>
        <a href="#top">top</a>
      </>
    ),
    "",
    CatchAll,
    createHashHistory(),
  );
  expect(stateOf(anchor("#/about"))).toBe("page active -");
  expect(stateOf(anchor("#top"))).toBe("- - -");
  go("/other");
  expect(stateOf(anchor("#/about"))).toBe("- - -");
});

test("an anchor to a loading route is pending until it commits, and a superseding navigation clears it", async () => {
  let module = Promise.withResolvers<RouteModule>();
  const routes: RouteDefinition[] = [
    { path: "/", component: Page },
    { path: "/lazy", load: () => module.promise },
  ];
  const { anchor, go } = setup(
    () => (
      <>
        <a href="/">home</a>
        <a href="/lazy">lazy</a>
      </>
    ),
    "/",
    routes,
  );
  go("/lazy");
  expect(stateOf(anchor("/lazy"))).toBe("- - pending");
  expect(stateOf(anchor("/"))).toBe("page active -");
  go("/");
  expect(stateOf(anchor("/lazy"))).toBe("- - -");
  module.resolve({ default: Page });
  await settle();
  expect(stateOf(anchor("/lazy"))).toBe("- - -");
  expect(stateOf(anchor("/"))).toBe("page active -");
  module = Promise.withResolvers<RouteModule>();
  go("/lazy");
  expect(stateOf(anchor("/lazy"))).toBe("page active -");
});

test("hrefs the router does not handle get no state", () => {
  const hrefs = ["#top", "?q=1", "https://other.test/", "//other.test/", "mailto:a@b.c", ""];
  const [isShown] = signal(true);
  const { el } = setup(() => hrefs.map((href) => <a href={isShown() ? href : undefined}>x</a>), "/");
  const anchors = el.querySelectorAll("nav a");
  expect(anchors).toHaveLength(hrefs.length);
  for (const a of anchors) expect(stateOf(a)).toBe("- - -");
});

test("hrefs outside the browser history base get no state", () => {
  window.history.replaceState(null, "", "/app/docs");
  const Router = createRouter({ routes: CatchAll, history: createBrowserHistory("/app") });
  const nav = mount(() => (
    <Router
      root={() => (
        <>
          <a href="/app/docs">in</a>
          <a href="/docs">out</a>
        </>
      )}
    />
  )).el;
  const [inside, outside] = nav.querySelectorAll("a");
  expect(stateOf(inside!)).toBe("page active -");
  expect(stateOf(outside!)).toBe("- - -");
});

test("anchors outside a router still bind a dynamic href and get no state", () => {
  const [to, setTo] = signal("/a");
  const { el } = mount(() => (
    <>
      <a href={to()}>x</a>
      <a href="/">y</a>
    </>
  ));
  const [dynamic, fixed] = el.querySelectorAll("a");
  expect(dynamic!.getAttribute("href")).toBe("/a");
  setTo("/b");
  tick();
  expect(dynamic!.getAttribute("href")).toBe("/b");
  expect(stateOf(dynamic!)).toBe("- - -");
  expect(stateOf(fixed!)).toBe("- - -");
});

test("a navigation writes attributes only on anchors whose state flipped", () => {
  const paths = Array.from({ length: 200 }, (_, i) => `/p${i}`);
  const { el, go } = setup(() => paths.map((path) => <a href={path}>{path}</a>), "/p0");
  const observer = new MutationObserver(() => {});
  observer.observe(el, { attributes: true, subtree: true });
  go("/p1");
  const changed = new Set(observer.takeRecords().map((record) => (record.target as Element).getAttribute("href")));
  observer.disconnect();
  expect(changed).toEqual(new Set(["/p0", "/p1"]));
});

test("useLinkState reports the same states as anchor attributes", async () => {
  const module = Promise.withResolvers<RouteModule>();
  const routes: RouteDefinition[] = [
    { path: "/blog/*rest", component: Page },
    { path: "/lazy", load: () => module.promise },
  ];
  const states: Record<string, LinkState> = {};
  const { go } = setup(
    () => {
      for (const href of ["/blog", "/blog/1", "/lazy"] as const) states[href] = useLinkState(() => href);
      return null;
    },
    "/blog/1",
    routes,
  );
  const read = (s: LinkState): string => [s.current() ? "page" : "-", s.active() ? "active" : "-", s.pending() ? "pending" : "-"].join(" ");
  expect(read(states["/blog"]!)).toBe("- active -");
  expect(read(states["/blog/1"]!)).toBe("page active -");
  go("/lazy");
  expect(read(states["/lazy"]!)).toBe("- - pending");
  module.resolve({ default: Page });
  await settle();
  expect(read(states["/lazy"]!)).toBe("page active -");
  expect(read(states["/blog"]!)).toBe("- - -");
});
