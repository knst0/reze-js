import { expect, test, vi } from "vitest";

import type { RouteDefinition } from "../src";
import {
  assertRedirectTarget,
  describeSsgRoutes,
  enumerateSsgUrls,
  isExternalRedirectTarget,
  normalizeSsgPath,
  prepareServerRoute,
  resolveSsgRedirectChain,
  resolveSsgRedirectTarget,
} from "../src/internal/server";

const Home = () => null;

test("describeSsgRoutes keeps file ids, assigns structural chains, and skips layouts", () => {
  const routes: RouteDefinition[] = [
    { id: "blog/[id]", path: "/blog/:id", component: Home },
    {
      path: "/docs",
      component: Home,
      children: [
        { path: "/guide", component: Home },
        { path: "/:topic?", component: Home },
      ],
    },
    { path: "/about", component: Home },
  ];
  expect(describeSsgRoutes(routes)).toEqual([
    { id: "blog/[id]", pattern: "/blog/:id", fullPath: "/blog/:id", dynamics: [{ name: "id", kind: "required" }] },
    { id: "1/0", pattern: "/docs/guide", fullPath: "/docs/guide", dynamics: [] },
    { id: "1/1", pattern: "/docs/:topic?", fullPath: "/docs/:topic?", dynamics: [{ name: "topic", kind: "optional" }] },
    { id: "2", pattern: "/about", fullPath: "/about", dynamics: [] },
  ]);
});

test("describeSsgRoutes rejects duplicate leaf patterns", () => {
  const routes: RouteDefinition[] = [
    { path: "/a", component: Home },
    { path: "/a", component: Home },
  ];
  expect(() => describeSsgRoutes(routes)).toThrow('share pattern "/a"');
});

test("enumerateSsgUrls lists static leaves once and expands dynamics per leaf", () => {
  const routes: RouteDefinition[] = [
    { path: "/", component: Home },
    { path: "/blog/:id", component: Home },
    { path: "/docs/*path", component: Home },
  ];
  const descriptors = describeSsgRoutes(routes);
  const urls = enumerateSsgUrls(
    descriptors,
    {
      "/blog/:id": [{ id: "one" }, { id: "two" }],
      "/docs/*path": [{ path: ["a", "b"] }],
    },
    { trailingSlash: "always" },
  );
  expect(urls).toEqual([
    { url: "/", file: "index.html", leafId: "0", params: {} },
    { url: "/blog/one/", file: "blog/one/index.html", leafId: "1", params: { id: "one" } },
    { url: "/blog/two/", file: "blog/two/index.html", leafId: "1", params: { id: "two" } },
    { url: "/docs/a/b/", file: "docs/a/b/index.html", leafId: "2", params: { path: ["a", "b"] } },
  ]);
  const never = enumerateSsgUrls(descriptors, { "/blog/:id": [{ id: "one" }], "/docs/*path": [] }, { trailingSlash: "never" });
  expect(never.map((entry) => entry.url)).toEqual(["/", "/blog/one"]);
});
test("enumerateSsgUrls rejects unknown keys, static entries, bad params, and collisions", () => {
  const descriptors = describeSsgRoutes([
    { path: "/", component: Home },
    { path: "/blog/:id", component: Home },
  ]);
  expect(() => enumerateSsgUrls(descriptors, { "/missing": [] }, { trailingSlash: "always" })).toThrow("unknown route pattern");
  expect(() => enumerateSsgUrls(descriptors, { "/": [] }, { trailingSlash: "always" })).toThrow("static route");
  expect(() => enumerateSsgUrls(descriptors, { "/blog/:id": [{ id: "x", extra: "y" }] }, { trailingSlash: "always" })).toThrow(
    "unknown param",
  );
  expect(() => enumerateSsgUrls(descriptors, {}, { trailingSlash: "always" })).toThrow("missing dynamic route");
  expect(() => enumerateSsgUrls(descriptors, { "/blog/:id": [{}] }, { trailingSlash: "always" })).toThrow("missing required param");
  expect(() => enumerateSsgUrls(descriptors, { "/blog/:id": [{ id: "a b" }, { id: "a%20b" }] }, { trailingSlash: "always" })).toThrow(
    "duplicate output",
  );
});

test("enumerateSsgUrls rejects an empty page set and decoded collisions", () => {
  const dynamicOnly = describeSsgRoutes([{ path: "/blog/:id", component: Home }]);
  expect(() => enumerateSsgUrls(dynamicOnly, { "/blog/:id": [] }, { trailingSlash: "always" })).toThrow("enumerates no pages");
  const encodedPair = describeSsgRoutes([
    { path: "/caf%C3%A9", component: Home },
    { path: "/caf\u00e9", component: Home },
  ]);
  expect(() => enumerateSsgUrls(encodedPair, {}, { trailingSlash: "always" })).toThrow("colliding URLs");
});

test("normalizeSsgPath accepts pathnames and rejects everything else", () => {
  expect(normalizeSsgPath("/")).toBe("/");
  expect(normalizeSsgPath("/blog/one")).toBe("/blog/one");
  expect(normalizeSsgPath("/blog/one/")).toBe("/blog/one");
  expect(() => normalizeSsgPath("blog/one")).toThrow("absolute router path");
  expect(() => normalizeSsgPath("/blog?x=1")).toThrow("excludes query and hash");
  expect(() => normalizeSsgPath("/blog/%2F")).toThrow("encoded slash");
  expect(() => normalizeSsgPath("/con")).toThrow("reserved");
});

test("redirect targets resolve relatively and classify schemes", () => {
  expect(resolveSsgRedirectTarget("/a/b/", "../c")).toBe("/a/c");
  expect(resolveSsgRedirectTarget("/a/b", "../c")).toBe("/c");
  expect(resolveSsgRedirectTarget("/a/b", "/c?x=1#h")).toBe("/c?x=1#h");
  expect(resolveSsgRedirectTarget("/a", "https://other.example/x")).toBe("https://other.example/x");
  expect(isExternalRedirectTarget("https://other.example/x")).toBe(true);
  expect(isExternalRedirectTarget("//other.example/x")).toBe(true);
  expect(isExternalRedirectTarget("/local")).toBe(false);
  expect(() => assertRedirectTarget("javascript:alert(1)")).toThrow("unsupported scheme");
  expect(() => assertRedirectTarget("data:text/plain,x")).toThrow("unsupported scheme");
  assertRedirectTarget("https://other.example/x");
  assertRedirectTarget("/local");
});

test("prepareServerRoute settles preloads and merges metadata root-to-leaf", async () => {
  const routes: RouteDefinition[] = [
    {
      id: "root",
      path: "/",
      component: Home,
      preload: () => Promise.resolve("root-data"),
      meta: { title: "Root", description: "root desc" },
      children: [
        {
          id: "post",
          path: "/blog/:id",
          component: Home,
          preload: ({ params }) => ({ title: `Post ${params.id}` }),
          meta: ({ data }) => ({ title: data.title }),
        },
      ],
    },
  ];
  const prepared = await prepareServerRoute(routes, "/blog/7");
  expect(prepared.status).toBe("render");
  if (prepared.status !== "render") return;
  expect(prepared.metadata).toEqual({ title: "Post 7", description: "root desc" });
  expect(prepared.matches).toEqual([
    { id: "root", params: { id: "7" }, hasData: true, data: "root-data" },
    { id: "post", params: { id: "7" }, hasData: true, data: { title: "Post 7" } },
  ]);
});

test("prepareServerRoute distinguishes missing preloads from undefined data", async () => {
  const routes: RouteDefinition[] = [
    {
      id: "root",
      path: "/",
      component: Home,
      preload: () => undefined,
      children: [{ id: "plain", path: "/plain", component: Home }],
    },
  ];
  const prepared = await prepareServerRoute(routes, "/plain");
  if (prepared.status !== "render") throw new Error("expected render");
  expect(prepared.matches).toEqual([
    { id: "root", params: {}, hasData: true, data: undefined },
    { id: "plain", params: {}, hasData: false },
  ]);
});

test("prepareServerRoute short-circuits literal redirects before preloads", async () => {
  const preload = vi.fn(() => "data");
  const routes: RouteDefinition[] = [
    { path: "/old", redirect: { to: "/new" }, preload, component: Home },
    { path: "/new", component: Home },
  ];
  const prepared = await prepareServerRoute(routes, "/old");
  expect(prepared).toEqual({ status: "redirect", to: "/new", replace: true });
  expect(preload).not.toHaveBeenCalled();
});

test("prepareServerRoute follows callback redirects with settled data", async () => {
  const seen: unknown[] = [];
  const routes: RouteDefinition[] = [
    {
      path: "/gated",
      component: Home,
      preload: () => Promise.resolve("go"),
      redirect: (args) => {
        seen.push(args.data);
        return { to: "/new" };
      },
    },
    { path: "/new", component: Home },
  ];
  const prepared = await prepareServerRoute(routes, "/gated");
  expect(prepared).toEqual({ status: "redirect", to: "/new", replace: true });
  expect(seen).toEqual(["go"]);
});

test("prepareServerRoute reports not-found for unknown paths", async () => {
  const prepared = await prepareServerRoute([{ path: "/", component: Home }], "/missing");
  expect(prepared).toEqual({ status: "not-found" });
});

test("resolveSsgRedirectChain reaches the render and detects cycles", async () => {
  const routes: RouteDefinition[] = [
    { path: "/a", redirect: { to: "/b" }, component: Home },
    { path: "/b", redirect: { to: "/c" }, component: Home },
    { path: "/c", component: Home, meta: { title: "C" } },
  ];
  const done = await resolveSsgRedirectChain(routes, "/a");
  if (done.status !== "render") throw new Error("expected render");
  expect(done.metadata).toEqual({ title: "C" });
  expect(done.matches.map((match) => match.id)).toEqual(["2"]);
  const loop: RouteDefinition[] = [
    { path: "/a", redirect: { to: "/b" }, component: Home },
    { path: "/b", redirect: { to: "/a" }, component: Home },
  ];
  await expect(resolveSsgRedirectChain(loop, "/a")).rejects.toThrow("redirect cycle detected: /a -> /b -> /a");
  const external: RouteDefinition[] = [{ path: "/out", redirect: { to: "https://other.example/x" }, component: Home }];
  await expect(resolveSsgRedirectChain(external, "/out")).resolves.toEqual({ status: "external", to: "https://other.example/x" });
});
