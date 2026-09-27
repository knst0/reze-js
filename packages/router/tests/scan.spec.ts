import { describe, expect, test } from "vitest";

import { scanRoutes, type FileRoute } from "../src/vite/scan";

type Shape = { id: string; path: string; fullPath: string; children?: Shape[] };

function shape(routes: FileRoute[]): Shape[] {
  return routes.map(({ id, path, fullPath, children }) =>
    children.length > 0 ? { id, path, fullPath, children: shape(children) } : { id, path, fullPath },
  );
}

describe("scanRoutes", () => {
  test("maps every segment kind and nests layouts", () => {
    const routes = scanRoutes([
      "index.tsx",
      "about.tsx",
      "blog.tsx",
      "blog/index.tsx",
      "blog/[id].tsx",
      "archive/[[page]].tsx",
      "docs/[...path].tsx",
      "(marketing)/pricing.tsx",
    ]);
    expect(shape(routes)).toEqual([
      { id: "(marketing)/pricing", path: "/pricing", fullPath: "/pricing" },
      { id: "about", path: "/about", fullPath: "/about" },
      { id: "archive/[[page]]", path: "/archive/:page?", fullPath: "/archive/:page?" },
      {
        id: "blog",
        path: "/blog",
        fullPath: "/blog",
        children: [
          { id: "blog/[id]", path: "/:id", fullPath: "/blog/:id" },
          { id: "blog/index", path: "/", fullPath: "/blog" },
        ],
      },
      { id: "docs/[...path]", path: "/docs/*path", fullPath: "/docs/*path" },
      { id: "index", path: "/", fullPath: "/" },
    ]);
  });

  test("a group file is a pathless layout for its directory", () => {
    expect(shape(scanRoutes(["(auth).tsx", "(auth)/login.tsx"]))).toEqual([
      { id: "(auth)", path: "/", fullPath: "/", children: [{ id: "(auth)/login", path: "/login", fullPath: "/login" }] },
    ]);
  });

  test("skips non-route files, declarations, tests and dot paths", () => {
    const routes = scanRoutes(["a.tsx", "b.css", "c.d.ts", "d.test.tsx", "e.spec.ts", ".hidden/f.tsx", "g/.h.tsx"]);
    expect(routes.map((route) => route.file)).toEqual(["a.tsx"]);
  });

  test.each([
    [["a.tsx", "a.ts"], '[reze-router] duplicate route files for "a": a.ts, a.tsx'],
    [["about.tsx", "(x)/about.tsx"], '[reze-router] routes "(x)/about.tsx" and "about.tsx" both match "/about"'],
    [["[...rest].tsx", "[...rest]/x.tsx"], '[reze-router] [...rest]/x.tsx: splat "rest" must be the last segment in "/*rest/x"'],
    [["[id].tsx", "[id]/[id].tsx"], '[reze-router] [id]/[id].tsx: duplicate param "id" in "/:id/:id"'],
    [["[1x].tsx"], '[reze-router] [1x].tsx: invalid param name "1x" in "/:1x"'],
  ])("%j throws", (files, message) => {
    expect(() => scanRoutes(files)).toThrow(message);
  });
});
