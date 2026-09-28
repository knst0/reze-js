import { describe, expect, test } from "vitest";

import { compileRoutes, matchBranches, matchPath } from "../src/match";
import type { RouteDefinition } from "../src/types";

function leafPath(defs: RouteDefinition[], pathname: string): string | undefined {
  return matchBranches(compileRoutes(defs), pathname)?.branch.routes.at(-1)?.def.path;
}

describe("ranking", () => {
  test("static beats param regardless of order", () => {
    expect(leafPath([{ path: "/users/:id" }, { path: "/users/new" }], "/users/new")).toBe("/users/new");
    expect(leafPath([{ path: "/users/:id" }, { path: "/users/new" }], "/users/7")).toBe("/users/:id");
  });

  test("param beats splat", () => {
    expect(leafPath([{ path: "/docs/*" }, { path: "/docs/:id" }], "/docs/a")).toBe("/docs/:id");
    expect(leafPath([{ path: "/docs/*" }, { path: "/docs/:id" }], "/docs/a/b")).toBe("/docs/*");
  });

  test("root beats a catch-all", () => {
    expect(leafPath([{ path: "/*404" }, { path: "/" }], "/")).toBe("/");
    expect(leafPath([{ path: "/*404" }, { path: "/" }], "/x")).toBe("/*404");
  });

  test("an earlier static segment beats a param, even ahead of a splat", () => {
    expect(leafPath([{ path: "/:x" }, { path: "/a/*" }], "/a")).toBe("/a/*");
    expect(leafPath([{ path: "/a/:b/:c" }, { path: "/a/b/*" }], "/a/b/c")).toBe("/a/b/*");
    expect(leafPath([{ path: "/a/*" }, { path: "/a" }], "/a")).toBe("/a");
  });

  test("equal ranks keep definition order", () => {
    expect(leafPath([{ path: "/:a" }, { path: "/:b" }], "/x")).toBe("/:a");
  });

  test("__proto__ is rejected as a param or splat name", () => {
    expect(() => compileRoutes([{ path: "/:__proto__" }])).toThrow('bad param name "__proto__"');
    expect(() => compileRoutes([{ path: "/*__proto__" }])).toThrow('bad splat name "__proto__"');
  });
});

describe("matchBranches", () => {
  test("static segments are case-insensitive and slashes are normalised", () => {
    const match = matchBranches(compileRoutes([{ path: "/users/:id" }]), "/Users/42/");
    expect(match?.params).toEqual({ id: "42" });
    expect(match?.path).toBe("/Users/42");
  });

  test("static segments match percent-encoded pathnames, as browsers report them", () => {
    const branches = compileRoutes([{ path: "/über/a b" }]);
    expect(matchBranches(branches, new URL("http://r/%C3%9Cber/a%20b").pathname)?.path).toBe("/%C3%9Cber/a%20b");
    expect(matchBranches(branches, "/über/a b")).toBeDefined();
    expect(matchPath("/über/*", "/%C3%BCber/x")?.path).toBe("/%C3%BCber");
  });

  test("params decode, keeping malformed escapes raw", () => {
    const branches = compileRoutes([{ path: "/p/:v" }]);
    expect(matchBranches(branches, "/p/%E2%9C%93")?.params).toEqual({ v: "✓" });
    expect(matchBranches(branches, "/p/%E0%A4%A")?.params).toEqual({ v: "%E0%A4%A" });
  });

  test("splat captures the rest, possibly empty", () => {
    const branches = compileRoutes([{ path: "/docs/*path" }]);
    expect(matchBranches(branches, "/docs")?.params).toEqual({ path: "" });
    expect(matchBranches(branches, "/docs/a/b")?.params).toEqual({ path: "a/b" });
  });

  test("optional params match with and without the segment", () => {
    const branches = compileRoutes([{ path: "/blog/:page?" }]);
    expect(matchBranches(branches, "/blog")?.params).toEqual({});
    expect(matchBranches(branches, "/blog/2")?.params).toEqual({ page: "2" });
    expect(matchBranches(branches, "/blog/2/3")).toBeUndefined();
  });

  test("a layout with children matches only through its children", () => {
    const branches = compileRoutes([{ path: "/blog", children: [{ path: "/:id" }] }]);
    expect(matchBranches(branches, "/blog")).toBeUndefined();
    const match = matchBranches(branches, "/blog/1");
    expect(match?.branch.routes.map((r) => r.def.path)).toEqual(["/blog", "/:id"]);
    expect(match?.params).toEqual({ id: "1" });
  });

  test("a child at / completes its parent path", () => {
    const branches = compileRoutes([{ path: "/blog", children: [{ path: "/" }] }]);
    expect(matchBranches(branches, "/blog")?.branch.routes).toHaveLength(2);
  });
});

describe("invalid patterns", () => {
  test.each([["/a/*rest/b"], ["/:1bad"], ["/:a?/b"]])("%s throws", (path) => {
    expect(() => compileRoutes([{ path }])).toThrow(`[reze-router] invalid route path "${path}"`);
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
