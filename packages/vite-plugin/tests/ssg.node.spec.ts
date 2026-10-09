import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import mdx from "@mdx-js/rollup";
import { parse, serialize } from "parse5";
import type { DefaultTreeAdapterTypes } from "parse5";
import type { Browser, Page } from "playwright";
import remarkFrontmatter from "remark-frontmatter";
import remarkMdxFrontmatter from "remark-mdx-frontmatter";
import { createServer, type Plugin } from "vite";
import { afterAll, beforeAll, describe, expect, test } from "vitest";

import reze, { DEFAULT_ROUTE_EXTENSIONS } from "../src/index";
import {
  buildSsgFixture,
  expectBuildFails,
  fetchHtml,
  launchEngine,
  listBuiltFiles,
  openPage,
  readBuiltFile,
  readManifest,
  resolveBrowsers,
  serveDist,
  waitFor,
} from "./ssg-harness";
import type { SsgBrowserName, StaticOrigin } from "./ssg-harness";

const FIXTURES = join(import.meta.dirname, "fixtures", "ssg");
const ROUTER_PATHS = {
  "/blog/:id": [{ id: "a" }, { id: "b" }],
  "/opt/:id?": [{}, { id: "x" }],
  "/files/*rest": [{ rest: ["a", "b"] }],
  "/ghost/:id": [],
} as const;

function fixture(name: string): string {
  return join(FIXTURES, name);
}

function attr(node: DefaultTreeAdapterTypes.Element | undefined, name: string): string | undefined {
  return node?.attrs.find((attribute) => attribute.name === name)?.value;
}

function elements(
  source: string | DefaultTreeAdapterTypes.Node,
  tag: string,
  attributes: Record<string, string> = {},
): DefaultTreeAdapterTypes.Element[] {
  const matches: DefaultTreeAdapterTypes.Element[] = [];
  const expected = Object.entries(attributes);
  const visit = (node: DefaultTreeAdapterTypes.Node): void => {
    if ("tagName" in node && node.tagName === tag && expected.every(([name, value]) => attr(node, name) === value)) matches.push(node);
    if ("childNodes" in node) for (const child of node.childNodes) visit(child);
  };
  visit(typeof source === "string" ? parse(source) : source);
  return matches;
}

function text(node: DefaultTreeAdapterTypes.Node | undefined): string {
  if (node === undefined) throw new Error("Expected a rendered element");
  if ("value" in node) return node.value;
  if (node.nodeName === "script" || node.nodeName === "style") return "";
  return "childNodes" in node ? node.childNodes.map(text).join("") : "";
}

function titleOf(html: string): string {
  return text(elements(html, "title")[0]);
}

function metaOf(html: string, name: string): string | undefined {
  return attr(elements(html, "meta", { name })[0], "content");
}

function canonicalOf(html: string): string | undefined {
  return attr(elements(html, "link", { rel: "canonical" })[0], "href");
}

function modulePreloads(html: string): string[] {
  return elements(html, "link", { rel: "modulepreload" }).map((node) => attr(node, "href")!);
}

function bodyText(html: string): string {
  return text(elements(html, "body")[0]);
}

function imgSrc(html: string, id?: string): string {
  return attr(elements(html, "img", id === undefined ? {} : { id })[0], "src") ?? "";
}

function stylesheetHrefs(html: string): string[] {
  return elements(html, "link", { rel: "stylesheet" }).map((node) => attr(node, "href")!);
}

async function clickUntil(page: Page, selector: string, predicate: string, tries = 60): Promise<void> {
  for (let attempt = 0; attempt < tries; attempt++) {
    await page.click(selector);
    if (await page.evaluate(predicate)) return;
    await page.waitForTimeout(200);
  }
  throw new Error(`[reze-test] ${selector} never produced the committed state`);
}

const ENGINES = resolveBrowsers();
const OUT = mkdtempSync(join(tmpdir(), "reze-ssg-"));
const dist = (name: string): string => join(OUT, name);
const origins: StaticOrigin[] = [];
const browsers = new Map<SsgBrowserName, Browser>();
const pages: Record<string, string> = {};

async function serve(distDir: string): Promise<string> {
  const origin = await serveDist(distDir);
  origins.push(origin);
  return origin.origin;
}

beforeAll(async () => {
  pages.basics = await serve(
    (
      await buildSsgFixture({
        fixtureDir: fixture("standalone-basics"),
        outDir: dist("basics"),
        plugins: reze({ ssg: { entry: "src/app.tsx" } }),
      })
    ).distDir,
  );
  pages.basicsCsr = await serve(
    (
      await buildSsgFixture({
        fixtureDir: fixture("standalone-basics"),
        outDir: dist("basics-csr"),
        htmlInput: "csr.html",
        plugins: reze(),
      })
    ).distDir,
  );
  pages.flows = await serve(
    (
      await buildSsgFixture({
        fixtureDir: fixture("flow-keys"),
        outDir: dist("flows"),
        plugins: reze({ ssg: { entry: "src/app.tsx" } }),
      })
    ).distDir,
  );
  pages.namespaces = await serve(
    (
      await buildSsgFixture({
        fixtureDir: fixture("namespaces"),
        outDir: dist("namespaces"),
        plugins: reze({ ssg: { entry: "src/app.tsx" } }),
      })
    ).distDir,
  );
  pages.portals = await serve(
    (
      await buildSsgFixture({
        fixtureDir: fixture("portals-islands"),
        outDir: dist("portals"),
        plugins: reze({ ssg: { entry: "src/app.tsx" } }),
      })
    ).distDir,
  );
  const routerPlugins = reze({
    ssg: { entry: "src/app.tsx", paths: ROUTER_PATHS },
  });
  pages.router = await serve(
    (await buildSsgFixture({ fixtureDir: fixture("router-full"), outDir: dist("router"), plugins: routerPlugins })).distDir,
  );
  pages.routerNever = (
    await buildSsgFixture({
      fixtureDir: fixture("router-full"),
      outDir: dist("router-never"),
      plugins: reze({ ssg: { entry: "src/app.tsx", paths: ROUTER_PATHS, trailingSlash: "never" } }),
    })
  ).distDir;
  pages.tinyDocs = (
    await buildSsgFixture({
      fixtureDir: fixture("base-tiny"),
      outDir: dist("tiny-docs"),
      base: "/docs/",
      plugins: reze({ ssg: { entry: "src/app.tsx" } }),
    })
  ).distDir;
  pages.tinyRelative = (
    await buildSsgFixture({
      fixtureDir: fixture("base-tiny"),
      outDir: dist("tiny-relative"),
      base: "./",
      plugins: reze({ ssg: { entry: "src/app.tsx" } }),
    })
  ).distDir;
  pages.tinyRelativeNever = (
    await buildSsgFixture({
      fixtureDir: fixture("base-tiny"),
      outDir: dist("tiny-relative-never"),
      base: "./",
      plugins: reze({ ssg: { entry: "src/app.tsx", trailingSlash: "never" } }),
    })
  ).distDir;
  pages.tinyExternal = (
    await buildSsgFixture({
      fixtureDir: fixture("base-tiny"),
      outDir: dist("tiny-external"),
      base: "https://cdn.example.test/assets/",
      plugins: reze({ ssg: { entry: "src/app.tsx" } }),
    })
  ).distDir;
  const mdxPlugin: Plugin = {
    ...mdx({
      jsx: true,
      jsxImportSource: "reze-js",
      providerImportSource: "/src/mdx",
      remarkPlugins: [remarkFrontmatter, remarkMdxFrontmatter],
    }),
    enforce: "pre",
  };
  pages.mdx = await serve(
    (
      await buildSsgFixture({
        fixtureDir: fixture("mdx-post"),
        outDir: dist("mdx"),
        plugins: [
          mdxPlugin,
          ...(await reze({
            fileRoutes: { types: false },
            extensions: [...DEFAULT_ROUTE_EXTENSIONS, ".mdx"],
            ssg: { entry: "src/app.tsx" },
          })),
        ],
      })
    ).distDir,
  );
  for (const engine of ENGINES) browsers.set(engine, await launchEngine(engine));
}, 600_000);

afterAll(async () => {
  for (const origin of origins) await origin.close();
  for (const browser of browsers.values()) await browser.close();
  rmSync(OUT, { recursive: true, force: true });
});

describe("preboot html, metadata, assets and lazy execution", () => {
  test("a standalone page carries settled signals, effects, async data and assets without js", async () => {
    const { status, html } = await fetchHtml(pages.basics, "/");
    expect(status).toBe(200);
    const clean = bodyText(html);
    expect(clean).toContain("basics");
    expect(clean).toContain("3:6:v3");
    expect(clean).toContain("settled quote");
    expect(clean).toContain("late arrived");
    expect(clean).not.toContain("loading quote…");
    expect(clean).not.toContain("loading late…");
    expect(text(elements(html, "output", { id: "spread-reads" })[0])).toBe("1");
    expect(titleOf(html)).toBe("Basics");
    expect(metaOf(html, "description")).toBe("Standalone basics fixture");
    expect(canonicalOf(html)).toBe("https://example.test/");
    for (const href of stylesheetHrefs(html)) {
      expect((await fetch(`${pages.basics}${href}`)).status).toBe(200);
    }
    const logo = await fetch(new URL(imgSrc(html, "logo"), pages.basics));
    expect(logo.status).toBe(200);
    expect(logo.headers.get("content-type")).toContain("image/svg+xml");
    expect(html).not.toContain("data-reze-state");
    expect(html).not.toContain("data-rz-patch");
  });

  test.each(ENGINES)("client-owned inline and emitted URLs load before and after boot on %s", async (engine) => {
    const { page, errors, close } = await openPage(browsers.get(engine)!, `${pages.router}/assets/`, {
      beforeHydration: async (page) => {
        await page.waitForFunction(`document.readyState !== "loading"`);
        const sources = await page.locator("article img").evaluateAll((images) => images.map((image) => image.getAttribute("src")));
        expect(sources[0]).toMatch(/^data:image\/svg\+xml,/);
        expect(sources[1]).toBe(sources[0]);
        expect(sources[2]).toMatch(/^\/assets\/logo-[^/]+\.svg$/);
        expect(sources[3]).toBe(sources[2]);
        expect(sources[4]).toBe(sources[0]);
        await waitFor(page, `[...document.querySelectorAll("article img")].every(image => image.complete && image.naturalWidth === 6)`);
        await page.evaluate(
          `window.__assetSnapshot = [...document.querySelectorAll("article img")].map(node => ({ node, src: node.src }))`,
        );
      },
    });
    try {
      expect(
        await page.evaluate(
          `window.__assetSnapshot.every(({ node, src }) => node === document.getElementById(node.id) && node.src === src && node.naturalWidth === 6)`,
        ),
      ).toBe(true);
      await page.click('nav a[href="/about"]');
      await waitFor(page, `document.getElementById("about-title") !== null`);
      expect(await page.evaluate(`window.__assetSnapshot !== undefined`)).toBe(true);
      expect(errors).toEqual([]);
    } finally {
      await close();
    }
  });

  test("router pages render layouts, settled preloads and merged head without js", async () => {
    const home = await fetchHtml(pages.router, "/");
    expect(home.status).toBe(200);
    expect(home.html).toContain("home-data");
    expect(titleOf(home.html)).toBe("Home");
    expect(metaOf(home.html, "description")).toBe("home page");
    const about = await fetchHtml(pages.router, "/about/");
    expect(about.status).toBe(200);
    expect(titleOf(about.html)).toBe("About");
    expect(canonicalOf(about.html)).toBe("https://example.test/about");
    expect(metaOf(about.html, "description")).toBe("Site template");
    expect(canonicalOf(home.html)).toBeUndefined();
    const post = await fetchHtml(pages.router, "/blog/a/");
    expect(post.status).toBe(200);
    expect(post.html).toContain("Post a");
    expect(titleOf(post.html)).toBe("Post a");
    const deep = await fetchHtml(pages.router, "/blog/a");
    expect(deep.status).toBe(200);
    const intro = await fetchHtml(pages.router, "/docs/intro/");
    expect(intro.html).toContain("docs");
    expect(intro.html).toContain("intro");
  });

  test("enumeration covers dynamics and skips layouts and empty path lists", async () => {
    expect((await fetchHtml(pages.router, "/opt/")).html).toContain("none");
    expect((await fetchHtml(pages.router, "/opt/x/")).html).toContain(">x<");
    expect((await fetchHtml(pages.router, "/files/a/b/")).html).toContain("a");
    expect((await fetchHtml(pages.router, "/docs/")).status).toBe(404);
    expect((await fetchHtml(pages.router, "/ghost/a/")).status).toBe(404);
    expect((await fetchHtml(pages.router, "/nope/")).status).toBe(404);
  });

  test("an unvisited lazy route ships no content, preload or initializer to other pages", async () => {
    const { html } = await fetchHtml(pages.router, "/");
    const manifest = readManifest(dist("router"));
    const lazyChunk = Object.values(manifest).find((chunk) => chunk.src?.includes("LazyPage"));
    expect(lazyChunk?.file).not.toBeUndefined();
    expect(html).not.toContain(lazyChunk!.file);
    expect(html).not.toContain("lazy loaded");
    expect(modulePreloads(html).some((href) => href.includes(lazyChunk!.file))).toBe(false);
    const ghostChunk = Object.values(manifest).find((chunk) => chunk.src?.includes("GhostPage"));
    expect(html).not.toContain("ghost");
    expect(
      ghostChunk === undefined || !modulePreloads(html).some((href) => ghostChunk.file !== undefined && href.includes(ghostChunk.file)),
    ).toBe(true);
    const lazy = await fetchHtml(pages.router, "/lazy/");
    expect(lazy.html).toContain("lazy loaded");
    expect(lazy.html).not.toContain("lazy-info-canary");
    expect(lazyChunk!.css).toHaveLength(1);
    for (const file of lazyChunk!.css!) {
      expect(stylesheetHrefs(html)).not.toContain(`/${file}`);
      expect(stylesheetHrefs(lazy.html)).toContain(`/${file}`);
    }
    expect(ghostChunk!.css).toHaveLength(1);
    for (const file of ghostChunk!.css!) {
      expect(stylesheetHrefs(html)).not.toContain(`/${file}`);
      expect(stylesheetHrefs(lazy.html)).not.toContain(`/${file}`);
    }
  });

  test.each(ENGINES)("executed lazy route CSS applies without JavaScript on %s", async (engine) => {
    const { page, close } = await openPage(browsers.get(engine)!, `${pages.router}/lazy/`, { javaScript: false });
    try {
      expect(await page.locator("#lazy-title").evaluate((node) => getComputedStyle(node).color)).toBe("rgb(17, 34, 51)");
    } finally {
      await close();
    }
  });

  test("route info stays code-owned and never enters an island descriptor", async () => {
    const { html } = await fetchHtml(pages.router, "/blog/a/");
    const descriptors = [...html.matchAll(/<script type="application\/json" data-rz-island[^>]*>([\s\S]*?)<\/script>/g)]
      .map((match) => match[1])
      .join("");
    expect(descriptors).not.toContain("blog-info-canary");
  });

  test("worker isolation keeps module state per page", async () => {
    expect(text(elements((await fetchHtml(pages.router, "/one/")).html, "p", { id: "hits" })[0])).toBe("1");
    expect(text(elements((await fetchHtml(pages.router, "/two/")).html, "p", { id: "hits" })[0])).toBe("1");
  });

  test("effect-derived state settles before serialization", async () => {
    const clean = bodyText((await fetchHtml(pages.router, "/effect/")).html);
    expect(clean).toContain("4:14");
    expect(clean).toContain("2/7");
  });

  test("await aliases keep identity with one mutation per execution", async () => {
    expect(bodyText((await fetchHtml(pages.router, "/alias/")).html)).toContain("same:m,n");
  });

  test("missing data and undefined data stay distinct", async () => {
    expect((await fetchHtml(pages.router, "/nodata/")).html).toContain("no data");
    expect((await fetchHtml(pages.router, "/undefdata/")).html).toContain("undef");
  });
});

describe("keyed flow lifetime", () => {
  test.each(ENGINES)(
    "rows keep identity across reorder, removal and addition on %s",
    async (engine) => {
      const browser = browsers.get(engine)!;
      const { page, errors, close } = await openPage(browser, `${pages.flows}/`);
      try {
        await waitFor(page, `document.querySelector("#deferred") !== null`);
        await page.evaluate(`(() => {
        window.__rowA = document.querySelector("#row-a");
        window.__rowB = document.querySelector("#row-b");
        document.querySelector("#row-b").value = "beta-user";
      })()`);
        await page.click("#reorder");
        await waitFor(page, `document.querySelectorAll("#rows li")[0].dataset.row === "b"`);
        const kept = await page.evaluate(`(() => ({
        a: window.__rowA === document.querySelector("#row-a"),
        b: window.__rowB === document.querySelector("#row-b"),
        value: document.querySelector("#row-b").value,
      }))()`);
        expect(kept).toEqual({ a: true, b: true, value: "beta-user" });
        await page.click("#drop-b");
        await waitFor(page, `document.querySelector("#removed").textContent === "b"`);
        expect(await page.evaluate(`document.querySelectorAll("#rows li").length`)).toBe(1);
        await page.click("#add-c");
        await waitFor(page, `document.querySelector("#row-c") !== null`);
        const added = await page.evaluate(`(() => ({
        a: window.__rowA === document.querySelector("#row-a"),
        rows: [...document.querySelectorAll("#rows li")].map((li) => li.dataset.row).join(","),
      }))()`);
        expect(added).toEqual({ a: true, rows: "a,c" });
        await page.click("#toggle-show");
        await waitFor(page, `document.querySelector("#show-fallback") !== null`);
        await page.click("#toggle-show");
        await waitFor(page, `document.querySelector("#show-body") !== null`);
        await page.click("#mode-two-btn");
        await waitFor(page, `document.querySelector("#mode-two") !== null`);
        await page.click("#fail-btn");
        await waitFor(page, `document.querySelector("#flow-error") !== null`);
        await page.click("#fail-btn");
        await page.click("#flow-reset");
        await waitFor(page, `document.querySelector("#fallible") !== null`);
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    90_000,
  );
});

describe("parser namespaces, raw text and opaque subtrees", () => {
  test("the built corpus carries parser-normalized topology", async () => {
    const { html } = await fetchHtml(pages.namespaces, "/");
    expect(elements(html, "tbody")).toHaveLength(1);
    expect(elements(html, "td").map((node) => text(node))).toEqual(["a1", "first"]);
    expect(
      elements(html, "option")
        .filter((node) => attr(node, "selected") !== undefined)
        .map((node) => attr(node, "value")),
    ).toEqual(["b"]);
    expect(text(elements(html, "textarea", { id: "notes" })[0])).toBe("first");
    expect(elements(html, "svg")[0]?.namespaceURI).toBe("http://www.w3.org/2000/svg");
    expect(elements(html, "foreignObject")[0]?.namespaceURI).toBe("http://www.w3.org/2000/svg");
    expect(elements(html, "math")[0]?.namespaceURI).toBe("http://www.w3.org/1998/Math/MathML");
    expect(text(elements(html, "b")[0])).toBe("trusted");
    expect(text(elements(html, "p", { id: "empty-dynamic" })[0])).toBe("");
  });

  test.each(ENGINES)(
    "namespaces, selection and opaque replacement behave on %s",
    async (engine) => {
      const browser = browsers.get(engine)!;
      const { page, errors, close } = await openPage(browser, `${pages.namespaces}/`);
      try {
        await waitFor(page, `document.querySelector("#formula") !== null`);
        const ns = await page.evaluate(`(() => ({
        svg: document.querySelector("#art").namespaceURI,
        circle: document.querySelector("#art circle").namespaceURI,
        math: document.querySelector("#formula").namespaceURI,
        option: document.querySelector("#picker").value,
      }))()`);
        expect(ns).toEqual({
          svg: "http://www.w3.org/2000/svg",
          circle: "http://www.w3.org/2000/svg",
          math: "http://www.w3.org/1998/Math/MathML",
          option: "b",
        });
        await page.selectOption("#picker", "a");
        await waitFor(page, `document.querySelector("#city-out").textContent === "a"`);
        await page.fill("#notes", "second");
        await waitFor(page, `document.querySelector("#grid td:last-child").textContent === "second"`);
        await page.click("#raw");
        await waitFor(page, `document.querySelector("#opaque").innerHTML === "<i>swapped</i>"`);
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    60_000,
  );

  test("unrepresentable nesting fails the build with a site", async () => {
    await expectBuildFails(
      reze({ ssg: { entry: "src/app.tsx" } }),
      fixture("foster-bad"),
      dist("foster-bad"),
      /table|foster|parent|serial/i,
    );
  });
});

describe("portals, islands and trigger cancellation", () => {
  test("the build adopts eager bodies and keeps deferred ones inert", async () => {
    const { html } = await fetchHtml(pages.portals, "/");
    expect(html).toContain("eager body");
    expect(html).toContain("clock waiting");
    expect(html).toContain("counter waiting");
    expect(html).toContain("cancel waiting");
    expect(html).not.toContain("data-ticked");
    expect(html).not.toContain("never body");
  });

  test.each(ENGINES)(
    "portals move and islands boot without remounting the page on %s",
    async (engine) => {
      const browser = browsers.get(engine)!;
      const { page, errors, close } = await openPage(browser, `${pages.portals}/`);
      try {
        await waitFor(page, `document.querySelector("#slot").contains(document.querySelector("#custom")) === true`);
        await page.click("#clock-fallback");
        await waitFor(page, `document.querySelector("#clock") !== null`);
        expect(await page.evaluate(`document.querySelector("#clock").getAttribute("data-ticked")`)).toBe("yes");
        await page.click("#counter-fallback");
        await waitFor(page, `document.querySelector("#counter-out") !== null`);
        await page.click("#counter-plus");
        await waitFor(page, `document.querySelector("#counter-out").textContent === "2"`);
        await page.click("#cancel");
        await waitFor(page, `document.querySelector("#cancel-fallback") === null`);
        expect(await page.evaluate(`document.body.textContent.includes("never body")`)).toBe(false);
        await page.click("#release");
        await waitFor(page, `document.querySelector("#custom")?.parentNode === document.body`);
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    90_000,
  );
});

describe("routing, enumeration, base and redirects", () => {
  test("redirects, chains, imperative navigation and externals build static documents", async () => {
    for (const path of ["/old/", "/chain/", "/cond/", "/admin/", "/redirect-only/"]) {
      const { status, html } = await fetchHtml(pages.router, path);
      expect(status).toBe(200);
      expect(html).not.toContain("@reze/client");
      expect(html).not.toContain("data-reze-state");
      expect(canonicalOf(html)).toBe(path === "/chain/" ? "/old/" : "/about/");
      const script = elements(html, "script").find((node) => node.attrs.some((attr) => attr.name === "data-reze-redirect"))!;
      const payload = JSON.parse((script.childNodes[0] as DefaultTreeAdapterTypes.TextNode).value);
      expect(payload.to).toBe(canonicalOf(html));
    }
    const external = await fetchHtml(pages.router, "/external/");
    expect(external.html).toContain("https://example.test/out");
    expect(external.html).not.toContain("@reze/client");
  });

  test("trailing slash selects generated urls while the file layout stays put", async () => {
    const always = await fetchHtml(pages.router, "/old/");
    const neverFiles = listBuiltFiles(dist("router-never"));
    expect(neverFiles).toContain("old/index.html");
    expect(neverFiles).toContain("about/index.html");
    expect(readBuiltFile(dist("router-never"), "old/index.html")).toContain('"/about"');
    expect(always.html).toContain('"/about/"');
  });

  test("enumeration, base and asset failures are build errors", async () => {
    await expectBuildFails(reze({ ssg: { entry: "src/app.tsx" } }), fixture("redirect-loop"), dist("neg-loop"), /loop|redirect|cycle/i);
    await expectBuildFails(
      reze({ ssg: { entry: "src/app.tsx" } }),
      fixture("redirect-missing"),
      dist("neg-missing"),
      /nowhere|redirect|target/i,
    );
    await expectBuildFails(
      reze({ ssg: { entry: "src/app.tsx", paths: { "/x/:id": [{ id: "a" }] } } }),
      fixture("paths-duplicate"),
      dist("neg-dup"),
      /duplicate|collide/i,
    );
    await expectBuildFails(
      reze({ ssg: { entry: "src/app.tsx", paths: { "/blog/:id": [{ id: "../evil" }] } } }),
      fixture("paths-unsafe"),
      dist("neg-unsafe"),
      /traverse|\.\.|unsafe|slash/i,
    );
  });

  test.each([
    ["tinyDocs", "/docs/"],
    ["tinyExternal", "https://cdn.example.test/assets/"],
  ])("absolute asset URLs resolve to emitted files for %s", (name, base) => {
    const directory = pages[name]!;
    const html = readBuiltFile(directory, "a/b/index.html");
    const scripts = elements(html, "script", { type: "module" }).map((node) => attr(node, "src")!);
    const styles = stylesheetHrefs(html);
    expect(scripts).toHaveLength(1);
    expect(styles).toHaveLength(1);
    const files = new Set(listBuiltFiles(directory));
    for (const url of [...scripts, ...styles, ...modulePreloads(html), imgSrc(html, "tiny-logo"), imgSrc(html, "tiny-public")]) {
      expect(url.startsWith(base)).toBe(true);
      expect(files.has(decodeURIComponent(url.slice(base.length)))).toBe(true);
    }
  });

  test.each(ENGINES)(
    "relative bases load nested assets and swap to the next page on %s",
    async (engine) => {
      const browser = browsers.get(engine)!;
      for (const [directory, pathname, prefix] of [
        [pages.tinyRelative!, "/a/b/", ""],
        [pages.tinyRelativeNever!, "/a/b", ""],
        [OUT, "/a/b/", "/tiny-relative"],
        [OUT, "/a/b", "/tiny-relative-never"],
      ] as const) {
        const origin = await serve(directory);
        const { page, errors, close } = await openPage(browser, `${origin}${prefix}${pathname}`);
        try {
          expect(await page.textContent("#package-badge")).toBe("linked package");
          expect(await page.getAttribute("#tiny-logo", "srcset")).toMatch(/logo%20caf%C3%A9-[^ ]+\.svg 1x, .* 2x$/);
          await waitFor(page, `[...document.querySelectorAll("img")].every(image => image.complete && image.naturalWidth === 6)`);
          expect(await page.textContent("#tiny-path")).toBe(pathname);
          expect(await page.evaluate(`getComputedStyle(document.querySelector("main")).color`)).toBe("rgb(51, 51, 51)");
          await page.evaluate(`globalThis.__marker = "kept"`);
          await clickUntil(page, "#tiny-go-home", `document.querySelector("#tiny-home") !== null`);
          expect(await page.textContent("#tiny-path")).toBe("/");
          expect(new URL(page.url()).pathname).toBe(`${prefix}/`);
          expect(await page.evaluate(`globalThis.__marker`)).toBe("kept");
          expect(errors).toEqual([]);
        } finally {
          await close();
        }
      }
    },
    90_000,
  );

  test("mdx routes carry frontmatter metadata and imported images", async () => {
    const { status, html } = await fetchHtml(pages.mdx, "/docs/guide/");
    expect(status).toBe(200);
    expect(titleOf(html)).toBe("Guide");
    expect(metaOf(html, "description")).toBe("mdx guide");
    expect(html).toContain("Guide");
    const img = await fetch(new URL(imgSrc(html), `${pages.mdx}/docs/guide/`));
    expect(img.status).toBe(200);
    expect(img.headers.get("content-type")).toContain("image/svg+xml");
  });

  test.each(ENGINES)(
    "links swap fetched pages, prefetch on hover, merge the head and keep the document on %s",
    async (engine) => {
      const browser = browsers.get(engine)!;
      const home = await openPage(browser, `${pages.router}/`);
      try {
        await clickUntil(home.page, "#home-inc", `document.querySelector("#home-inc").textContent === "1"`);
        expect(await home.page.evaluate(`document.title`)).toBe("Home");
        await home.page.evaluate(`globalThis.__marker = "kept"`);
        const blogRequests = (): string[] => home.requests.filter((url) => new URL(url).pathname.replace(/\/$/, "") === "/blog/a");
        await home.page.hover("#to-blog-a");
        await expect.poll(blogRequests).toHaveLength(1);
        await home.page.click("#to-blog-a");
        await waitFor(home.page, `document.querySelector("#post-title") !== null`);
        expect(await home.page.textContent("#post-title")).toBe("Post a");
        expect(blogRequests()).toHaveLength(1);
        await home.page.goBack();
        await waitFor(home.page, `document.querySelector("#to-about") !== null`);
        await home.page.click("#to-about");
        await waitFor(home.page, `document.querySelector("#about-title") !== null`);
        expect(await home.page.evaluate(`document.title`)).toBe("About");
        expect(metaOf(await home.page.content(), "description")).toBe("Site template");
        expect(await home.page.evaluate(`globalThis.__marker`)).toBe("kept");
        expect(home.errors).toEqual([]);
      } finally {
        await home.close();
      }
      const lazy = await openPage(browser, `${pages.router}/`);
      try {
        await waitFor(lazy.page, `document.querySelector("#to-lazy") !== null`);
        const manifest = readManifest(dist("router"));
        const lazyFile = Object.values(manifest).find((chunk) => chunk.src?.includes("LazyPage"))?.file;
        await lazy.page.click("#to-lazy");
        await waitFor(lazy.page, `document.querySelector("#lazy-title") !== null`);
        if (lazyFile !== undefined) expect(lazy.requests.some((url) => url.endsWith(lazyFile))).toBe(false);
        expect(lazy.errors).toEqual([]);
      } finally {
        await lazy.close();
      }
    },
    120_000,
  );

  test.each(ENGINES)(
    "redirect chains and imperative navigation land on %s",
    async (engine) => {
      const browser = browsers.get(engine)!;
      for (const path of ["/chain/", "/admin/"]) {
        const visit = await openPage(browser, `${pages.router}${path}`);
        try {
          await waitFor(visit.page, `document.querySelector("#about-title") !== null`);
          expect(visit.errors).toEqual([]);
        } finally {
          await visit.close();
        }
      }
    },
    60_000,
  );
});

describe("worker isolation, cleanup and timeouts", () => {
  test("a hung page fails with its url while other builds stay usable", async () => {
    await expectBuildFails(
      reze({ ssg: { entry: "src/app.tsx", timeoutMs: 500 } }),
      fixture("timeout-hang"),
      dist("neg-timeout"),
      /slow|timeout|deadline/i,
    );
    const { status, html } = await fetchHtml(pages.basics, "/");
    expect(status).toBe(200);
    expect(html).toContain("basics");
  });
});

test("disabled public files do not collide with the application's HTML template", async () => {
  const built = await buildSsgFixture({
    fixtureDir: fixture("standalone-basics"),
    outDir: dist("public-disabled"),
    plugins: [{ name: "public-disabled", config: () => ({ publicDir: false }) }, ...(await reze({ ssg: { entry: "src/app.tsx" } }))],
  });
  expect(text(elements(readBuiltFile(built.distDir, "index.html"), "p", { id: "settled" })[0])).toBe("3:6:v3");
});

describe("ordinary client-rendered production output", () => {
  test.each(ENGINES)("mounts an empty template and updates derived state on %s", async (engine) => {
    const { page, errors, close } = await openPage(browsers.get(engine)!, `${pages.basicsCsr}/csr.html`, {
      beforeHydration: async (page) => {
        await page.waitForFunction(`document.readyState !== "loading"`);
        expect(await page.locator("#app").evaluate((root) => root.childNodes.length)).toBe(0);
      },
    });
    try {
      await waitFor(page, `document.querySelector("#settled")?.textContent === "3:6:v3"`);
      expect(await page.textContent("#greeting")).toBe("hi basics");
      expect(await page.textContent("#spread-reads")).toBe("1");
      await page.click("#inc");
      await waitFor(page, `document.querySelector("#settled").textContent === "4:8:v4"`);
      await waitFor(page, `document.querySelector("#quote")?.textContent === "settled quote"`);
      expect(errors).toEqual([]);
    } finally {
      await close();
    }
  });
});

describe("development serves the streamed document", () => {
  test.each(ENGINES)(
    "standalone and router entries render on the server and boot islands in the browser on %s",
    async (engine) => {
      for (const [name, pathname, button, output, expected] of [
        ["standalone-basics", "/", "#inc", "#settled", "4:8:v4"],
        ["router-full", "/counter/", "#counter-inc", "#counter-out", "3:6"],
      ] as const) {
        const server = await createServer({
          configFile: false,
          root: fixture(name),
          cacheDir: join(OUT, `dev-${name}-${engine}`),
          plugins: [reze({ ssg: { entry: "src/app.tsx" } })],
          server: { host: "127.0.0.1", port: 0 },
          logLevel: "silent",
        });
        try {
          await server.listen();
          const url = new URL(pathname, server.resolvedUrls!.local[0]).href;
          const html = await (await fetch(url, { headers: { accept: "text/html" } })).text();
          expect(text(elements(html, "div", { id: "app" })[0])).not.toBe("");
          const visit = await openPage(browsers.get(engine)!, url);
          try {
            await clickUntil(
              visit.page,
              button,
              `document.querySelector(${JSON.stringify(output)}).textContent === ${JSON.stringify(expected)}`,
            );
            expect(visit.errors).toEqual([]);
          } finally {
            await visit.close();
          }
        } finally {
          await server.close();
        }
      }
    },
    90_000,
  );
});
