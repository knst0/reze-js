import { type JSX } from "reze-js";
import { afterEach, expect, test } from "vitest";

import { cleanup, mount, tick } from "../../../testing/dom";
import {
  buildPaths,
  createBrowserHistory,
  createRouter,
  createMemoryHistory,
  useNavigate,
  type Navigate,
  type RouteDefinition,
} from "../src";

afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
});

function call(node: unknown, ...args: readonly unknown[]): string {
  return (node as (...args: readonly unknown[]) => string)(...args);
}

const defs: RouteDefinition[] = [
  { path: "/", name: "index", component: () => <p>home</p> },
  { path: "/about", name: "about", component: () => <p>about</p> },
  {
    path: "/blog",
    name: "blog",
    children: [
      { path: "/", name: "index", component: () => <p>posts</p> },
      { path: "/:id", name: "byId", component: () => <p>post</p> },
    ],
  },
  {
    path: "/org/:orgId",
    name: "org",
    children: [{ path: "/repo/:name", name: "repo" }],
  },
  {
    path: "/admin",
    name: "admin",
    children: [{ path: "/users", name: "users" }],
  },
  { path: "/archive/:page?", name: "archive" },
  { path: "/docs/*path", name: "docs" },
];

test("statics build hrefs, params bind and encode, search and hash append", () => {
  const paths = buildPaths(defs);
  expect(call(paths.index)).toBe("/");
  expect(call(paths.about)).toBe("/about");
  expect(call(paths.about, { q: "x", n: 2 }, "sec")).toBe("/about?q=x&n=2#sec");
  expect(call(paths.blog.byId, 1)).toBe("/blog/1");
  expect(call(paths.blog.byId, "a/b")).toBe("/blog/a%2Fb");
  expect(call(paths.blog.index)).toBe("/blog");
  expect(call(paths.archive)).toBe("/archive");
  expect(call(paths.archive, 2)).toBe("/archive/2");
  expect(call(paths.docs)).toBe("/docs");
  expect(call(paths.docs, ["a", "b"])).toBe("/docs/a/b");
  expect(call(paths.docs, "a/b")).toBe("/docs/a/b");
  expect(call(paths.docs, ["a/b"])).toBe("/docs/a%2Fb");
});

test("param calls continue into nested nodes", () => {
  const paths = buildPaths(defs);
  expect(paths.org("acme").repo("reze")).toBe("/org/acme/repo/reze");
});

test("a layout without an index is a branch, not a call", () => {
  const paths = buildPaths(defs);
  expect(typeof paths.admin).toBe("object");
  expect(String(paths.admin)).toBe("/admin");
  expect(call(paths.admin.users)).toBe("/admin/users");
  expect(call(paths.blog)).toBe("/blog");
});

test("builders carry the served base", () => {
  expect(call(buildPaths(defs, "/app").about)).toBe("/app/about");
  expect(call(buildPaths(defs, "/app").blog.byId, 1)).toBe("/app/blog/1");
  expect(call(buildPaths(defs, "#").about)).toBe("#/about");
});

test("duplicate sibling names throw at build, multi-dynamic patterns on call", () => {
  expect(() => buildPaths([{ path: "/u", children: [{ path: "/byId" }, { path: "/:id" }] }])).toThrow(
    '[reze-router] duplicate path name "byId" under "/u"; rename a segment',
  );
  const multi = buildPaths([{ path: "/org/:id/repo/:name" }]);
  expect(() => call(multi.byName, "acme", "reze")).toThrow(
    '[reze-router] paths.byName has 2 dynamic segments; split "/org/:id/repo/:name" into nested routes',
  );
  expect(() => call(buildPaths(defs).blog.byId)).toThrow('[reze-router] paths.blog.byId requires a value for ":id"');
});

test("a definition name overrides the derived key", () => {
  const paths = buildPaths([{ path: "/", name: "auth", children: [{ path: "/login" }] }]);
  expect(call(paths.auth.login)).toBe("/login");
});

test("navigate accepts served-base hrefs from the builders", () => {
  window.history.replaceState(null, "", "/app/start");
  const history = createBrowserHistory("/app");
  const routes: RouteDefinition[] = [
    { path: "/", component: () => <p>home</p> },
    { path: "/about", component: () => <p>about</p> },
  ];
  const Router = createRouter({ routes, history, paths: buildPaths(routes, history.base) });
  let navigate!: Navigate;
  function Root(props: { children: JSX.Element }) {
    navigate = useNavigate();
    return <main>{props.children}</main>;
  }
  const { el } = mount(() => <Router root={Root} />);
  navigate(call(Router.paths.about));
  tick();
  expect(el.textContent).toBe("about");
  expect(history.get().path).toBe("/about");
});

test("match reports root-to-leaf patterns, params and info without rendering", () => {
  const Router = createRouter({
    routes: [
      {
        path: "/blog",
        component: () => <p>layout</p>,
        info: { section: "blog" },
        children: [{ path: "/:id", component: () => <p>post</p> }],
      },
      { path: "/about", component: () => <p>about</p> },
    ],
    history: createMemoryHistory("/"),
  });
  expect(Router.match("/blog/1?x=2")).toEqual([
    { path: "/blog/1", pattern: "/blog", params: { id: "1" }, info: { section: "blog" } },
    { path: "/blog/1", pattern: "/blog/:id", params: { id: "1" }, info: undefined },
  ]);
  expect(Router.match("/blog/a%2Fb")).toEqual([
    { path: "/blog/a%2Fb", pattern: "/blog", params: { id: "a/b" }, info: { section: "blog" } },
    { path: "/blog/a%2Fb", pattern: "/blog/:id", params: { id: "a/b" }, info: undefined },
  ]);
  expect(Router.match("/nope")).toEqual([]);
});
