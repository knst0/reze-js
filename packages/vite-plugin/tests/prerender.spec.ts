import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { expect, test, vi } from "vitest";

import { escapeText, injectShell, prerenderHtml, renderTree, type PrerenderModule, type PrerenderTree } from "../src/prerender";

function index(modules: Record<string, PrerenderModule>, resolved: Record<string, string> = {}) {
  return {
    getModule: (id: string) => modules[id],
    resolve: async (request: string, importer: string) => resolved[`${importer} › ${request}`],
  };
}

const empty = index({});

test("static trees pass through, escaping text", async () => {
  const tree: PrerenderTree = {
    Children: [{ Html: { html: "<p>hi", namespace: "html" } }, { Text: "a & <b>" }, "Empty"],
  };
  await expect(renderTree(tree, empty, "/src/main.tsx")).resolves.toBe("<p>hia &amp; &lt;b>");
});

test("escapeText mirrors the compiler", () => {
  expect(escapeText('a&b<c>d"e')).toBe('a&amp;b&lt;c>d"e');
});

test("holes resolve through same-module and cross-module components", async () => {
  const page: PrerenderTree = {
    Mixed: {
      html: "<main><!--reze0--><!--reze1--></main>",
      holes: [
        { id: 0, target: { request: null, path: ["Layout"] } },
        { id: 1, target: { request: "./Counter", path: ["Counter"] } },
      ],
    },
  };
  const modules: Record<string, PrerenderModule> = {
    "/src/main.tsx": {
      components: [{ name: "Layout", exported: [], tree: { Html: { html: "<nav>menu</nav>", namespace: "html" } } }],
      roots: [],
    },
    "/src/Counter.tsx": {
      components: [
        {
          name: "Counter",
          exported: ["Counter"],
          tree: { Html: { html: "<output>0</output>", namespace: "html" } },
        },
      ],
      roots: [],
    },
  };
  const stub = index(modules, { "/src/main.tsx › ./Counter": "/src/Counter.tsx" });
  await expect(renderTree(page, stub, "/src/main.tsx")).resolves.toBe("<main><nav>menu</nav><output>0</output></main>");
});

test("unresolvable holes and cycles render as nothing", async () => {
  const modules: Record<string, PrerenderModule> = {
    "/src/a.tsx": {
      components: [
        {
          name: "A",
          exported: ["A"],
          tree: {
            Mixed: {
              html: "<div><!--reze0--><!--reze1--></div>",
              holes: [
                { id: 0, target: { request: "./b", path: ["B"] } },
                { id: 1, target: { request: "./missing", path: ["Gone"] } },
              ],
            },
          },
        },
      ],
      roots: [],
    },
    "/src/b.tsx": {
      components: [
        {
          name: "B",
          exported: ["B"],
          tree: { Component: { request: "./a", path: ["A"] } },
        },
      ],
      roots: [],
    },
  };
  const stub = index(modules, {
    "/src/main.tsx › ./a": "/src/a.tsx",
    "/src/a.tsx › ./b": "/src/b.tsx",
    "/src/b.tsx › ./a": "/src/a.tsx",
  });
  const tree: PrerenderTree = { Component: { request: "./a", path: ["A"] } };
  await expect(renderTree(tree, stub, "/src/main.tsx")).resolves.toBe("<div></div>");
});

test("injectShell replaces the shell content", () => {
  const html = '<html><body><div id="app"></div><script src="/a.js"></script></body></html>';
  expect(injectShell(html, "#app", "<p>hi</p>")).toBe(
    '<html><body><div id="app"><p>hi</p></div><script src="/a.js"></script></body></html>',
  );
  expect(injectShell(html, "#missing", "<p>hi</p>")).toBeUndefined();
  expect(injectShell(html, ".app", "<p>hi</p>")).toBeUndefined();
});

test("prerenderHtml resolves the entry script and injects its shell", async () => {
  const dir = mkdtempSync(join(tmpdir(), "reze-prerender-"));
  try {
    mkdirSync(join(dir, "src"), { recursive: true });
    writeFileSync(join(dir, "src", "main.tsx"), "entry");
    writeFileSync(join(dir, "src", "Counter.tsx"), "counter");
    writeFileSync(
      join(dir, "index.html"),
      '<html><body><div id="app"></div><script type="module" src="/src/main.tsx"></script></body></html>',
    );
    const sidecars = new Map([
      [
        join(dir, "src", "main.tsx"),
        {
          components: [],
          roots: [
            {
              Mixed: {
                html: "<main><!--reze0--></main>",
                holes: [{ id: 0, target: { request: "./Counter", path: ["Counter"] } }],
              },
            },
          ],
        },
      ],
      [
        join(dir, "src", "Counter.tsx"),
        {
          components: [
            {
              name: "Counter",
              exported: ["Counter"],
              tree: { Html: { html: "<output>0</output>", namespace: "html" } },
            },
          ],
          roots: [],
        },
      ],
    ]);
    const warn = vi.fn();
    const html = '<html><body><div id="app"></div><script type="module" src="/assets/index.js"></script></body></html>';
    const updated = await prerenderHtml(html, join(dir, "index.html"), dir, sidecars, "#app", {
      warn,
    });
    expect(updated).toBe(
      '<html><body><div id="app"><main><output>0</output></main></div><script type="module" src="/assets/index.js"></script></body></html>',
    );
    expect(warn).not.toHaveBeenCalled();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("prerenderHtml leaves unknown entries and shells alone", async () => {
  const warn = vi.fn();
  await expect(prerenderHtml("<div></div>", "/root/index.html", "/root", new Map(), "#app", { warn })).resolves.toBeUndefined();
  await expect(
    prerenderHtml('<div id="app"></div>', "/root/index.html", "/root", new Map(), "#missing", {
      warn,
    }),
  ).resolves.toBeUndefined();
  expect(warn).not.toHaveBeenCalled();
});
