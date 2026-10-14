import { describe, expect, test } from "vite-plus/test";

import { compileRoutes, matchBranches, matchPath } from "../src/match";
import type { RouteDefinition } from "../src/types";

function leafPath(defs: RouteDefinition[], pathname: string): string | undefined {
  return matchBranches(compileRoutes(defs), pathname)?.branch.routes.at(-1)?.def.path;
}

describe("ranking", () => {
  test("static beats param beats splat, root beats a catch-all, and equal ranks keep definition order", () => {
    expect(leafPath([{ path: "/users/:id" }, { path: "/users/new" }], "/users/new")).toBe("/users/new");
    expect(leafPath([{ path: "/users/:id" }, { path: "/users/new" }], "/users/7")).toBe("/users/:id");
    expect(leafPath([{ path: "/docs/*" }, { path: "/docs/:id" }], "/docs/a")).toBe("/docs/:id");
    expect(leafPath([{ path: "/docs/*" }, { path: "/docs/:id" }], "/docs/a/b")).toBe("/docs/*");
    expect(leafPath([{ path: "/*404" }, { path: "/" }], "/")).toBe("/");
    expect(leafPath([{ path: "/*404" }, { path: "/" }], "/x")).toBe("/*404");
    expect(leafPath([{ path: "/:x" }, { path: "/a/*" }], "/a")).toBe("/a/*");
    expect(leafPath([{ path: "/a/:b/:c" }, { path: "/a/b/*" }], "/a/b/c")).toBe("/a/b/*");
    expect(leafPath([{ path: "/a/*" }, { path: "/a" }], "/a")).toBe("/a");
    expect(leafPath([{ path: "/:a" }, { path: "/:b" }], "/x")).toBe("/:a");
  });
});

describe("matchBranches", () => {
  test("normalises static segments, decodes params and splats, and matches a layout only through its children", () => {
    const users = matchBranches(compileRoutes([{ path: "/users/:id" }]), "/Users/42/");
    expect(users?.params).toEqual({ id: "42" });
    expect(users?.path).toBe("/Users/42");

    const encoded = compileRoutes([{ path: "/über/a b" }]);
    expect(matchBranches(encoded, new URL("http://r/%C3%9Cber/a%20b").pathname)?.path).toBe("/%C3%9Cber/a%20b");
    expect(matchBranches(encoded, "/über/a b")).toBeDefined();
    expect(matchPath("/über/*", "/%C3%BCber/x")?.path).toBe("/%C3%BCber");

    const param = compileRoutes([{ path: "/p/:v" }]);
    expect(matchBranches(param, "/p/%E2%9C%93")?.params).toEqual({ v: "✓" });
    expect(matchBranches(param, "/p/%E0%A4%A")?.params).toEqual({ v: "%E0%A4%A" });

    const splat = compileRoutes([{ path: "/docs/*path" }]);
    expect(matchBranches(splat, "/docs")?.params).toEqual({ path: "" });
    expect(matchBranches(splat, "/docs/a/b")?.params).toEqual({ path: "a/b" });

    const layout = compileRoutes([{ path: "/blog", children: [{ path: "/:id" }] }]);
    expect(matchBranches(layout, "/blog")).toBeUndefined();
    const match = matchBranches(layout, "/blog/1");
    expect(match?.branch.routes.map((r) => r.def.path)).toEqual(["/blog", "/:id"]);
    expect(match?.params).toEqual({ id: "1" });
  });
});

describe("invalid patterns", () => {
  test("malformed route paths and reserved param or splat names throw", () => {
    for (const path of ["/a/*rest/b", "/:1bad", "/:a?/b"]) {
      expect(() => compileRoutes([{ path }]), path).toThrow(`[reze-router] invalid route path "${path}"`);
    }
    expect(() => compileRoutes([{ path: "/:__proto__" }])).toThrow('bad param name "__proto__"');
    expect(() => compileRoutes([{ path: "/*__proto__" }])).toThrow('bad splat name "__proto__"');
  });
});

describe("matchPath", () => {
  test("exact by default, prefix with a trailing /*", () => {
    expect(matchPath("/blog", "/blog/1")).toBeUndefined();
    expect(matchPath("/blog/*", "/blog/1")).toEqual({ params: {}, path: "/blog" });
    expect(matchPath("/blog/*", "/blog")).toEqual({ params: {}, path: "/blog" });
    expect(matchPath("/blog/:id", "/blog/1")).toEqual({ params: { id: "1" }, path: "/blog/1" });
  });
});
