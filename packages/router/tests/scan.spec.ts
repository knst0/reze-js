import { describe, expect, test } from "vite-plus/test";

import { scanRoutes, type FileRoute } from "../src/fs/scan";

type Shape = { id: string; path: string; fullPath: string; children?: Shape[] };

function shape(routes: FileRoute[]): Shape[] {
  return routes.map(({ id, path, fullPath, children }) =>
    children.length > 0 ? { id, path, fullPath, children: shape(children) } : { id, path, fullPath },
  );
}

describe("scanRoutes", () => {
  test("maps segment kinds, nests layouts, treats group files as pathless layouts, and names nodes for builders", () => {
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
    expect(shape(scanRoutes(["(auth).tsx", "(auth)/login.tsx"]))).toEqual([
      { id: "(auth)", path: "/", fullPath: "/", children: [{ id: "(auth)/login", path: "/login", fullPath: "/login" }] },
    ]);

    const named = scanRoutes(["index.tsx", "(auth).tsx", "(auth)/login.tsx", "blog.tsx", "blog/[id].tsx", "docs/[...path].tsx"]);
    expect(named.map((route) => ({ [route.name]: route.children.map((child) => child.name) }))).toEqual([
      { auth: ["login"] },
      { blog: ["byId"] },
      { byPath: [] },
      { index: [] },
    ]);
  });

  test("skips non-route files and custom extensions scan as routes with the extension stripped", () => {
    const skipped = scanRoutes([
      "a.tsx",
      "b.css",
      "c.d.ts",
      "d.test.tsx",
      "e.spec.ts",
      ".hidden/f.tsx",
      "g/.h.tsx",
      "_utils.ts",
      "_components/a.tsx",
      "blog/_draft.tsx",
      "c.mdx",
    ]);
    expect(skipped.map((route) => route.file)).toEqual(["a.tsx"]);

    const routes = scanRoutes(["index.mdx", "guide.md", "blog/[id].mdx", "notes.txt"], { extensions: ["mdx", ".md"] });
    expect(shape(routes)).toEqual([
      {
        id: "blog/[id]",
        path: "/blog/:id",
        fullPath: "/blog/:id",
      },
      { id: "guide", path: "/guide", fullPath: "/guide" },
      { id: "index", path: "/", fullPath: "/" },
    ]);
    expect(routes.map((route) => route.file)).toEqual(["blog/[id].mdx", "guide.md", "index.mdx"]);
  });

  test("conflicting or invalid route file sets throw", () => {
    const cases: [files: string[], message: string][] = [
      [["a.tsx", "a.ts"], '[reze-router] duplicate route files for "a": a.ts, a.tsx'],
      [["[id].tsx", "[slug].tsx"], '[reze-router] routes "[id].tsx" and "[slug].tsx" both match "/:slug"'],
      [["(a).tsx", "(a)/Login.tsx", "login.tsx"], '[reze-router] routes "(a)/Login.tsx" and "login.tsx" both match "/login"'],
      [["blog/index.tsx", "blog/[[page]].tsx"], '[reze-router] routes "blog/[[page]].tsx" and "blog/index.tsx" both match "/blog"'],
      [["[...rest].tsx", "[...rest]/x.tsx"], '[reze-router] [...rest]/x.tsx: splat "rest" must be the last segment in "/*rest/x"'],
      [["[id].tsx", "[id]/[id].tsx"], '[reze-router] [id]/[id].tsx: duplicate param "id" in "/:id/:id"'],
      [["[1x].tsx"], '[reze-router] [1x].tsx: invalid param name "1x" in "/:1x"'],
    ];
    for (const [files, message] of cases) {
      expect(() => scanRoutes(files), JSON.stringify(files)).toThrow(message);
    }
  });
});
