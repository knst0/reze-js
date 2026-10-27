import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { Browser } from "playwright";
import { afterAll, beforeAll, expect, test } from "vite-plus/test";

import { launchEngine, openPage, resolveBrowsers, waitFor } from "./ssg-harness";
import type { SsgBrowserName } from "./ssg-harness";
import { buildSsrFixture, clickUntil, defineGlobals, readChunks, serveSsrApp, startDevServer } from "./ssr-harness";
import type { SsrBuild, SsrOrigin } from "./ssr-harness";

const FIXTURES = join(import.meta.dirname, "fixtures", "ssr");
const ENGINES = resolveBrowsers();
const TIMEOUT = 180_000;
const OUT = mkdtempSync(join(tmpdir(), "reze-ssr-"));
const browsers = new Map<SsgBrowserName, Browser>();
const origins: SsrOrigin[] = [];

let moduleState: SsrBuild;
let streaming: SsrBuild;
let islands: SsrBuild;
let context: SsrBuild;
let routerA: SsrBuild;
let routerB: SsrBuild;
let routerActive: SsrBuild;
let streamingOrigin: SsrOrigin;
let islandsOrigin: SsrOrigin;
let contextOrigin: SsrOrigin;
let routerOrigin: SsrOrigin;
let devOrigin: SsrOrigin;
let devRouterOrigin: SsrOrigin;

function fixture(name: string): string {
  return join(FIXTURES, name);
}

beforeAll(async () => {
  [moduleState, streaming, islands, context, routerA, routerB] = await Promise.all([
    buildSsrFixture({ fixtureDir: fixture("module-state"), outDir: join(OUT, "module-state") }),
    buildSsrFixture({ fixtureDir: fixture("streaming"), outDir: join(OUT, "streaming") }),
    buildSsrFixture({ fixtureDir: fixture("islands"), outDir: join(OUT, "islands") }),
    buildSsrFixture({ fixtureDir: fixture("context"), outDir: join(OUT, "context") }),
    buildSsrFixture({
      fixtureDir: fixture("router"),
      outDir: join(OUT, "router-a"),
      plugins: [defineGlobals({ __RZ_TEST_VERSION__: "one" })],
    }),
    buildSsrFixture({
      fixtureDir: fixture("router"),
      outDir: join(OUT, "router-b"),
      plugins: [defineGlobals({ __RZ_TEST_VERSION__: "two" })],
    }),
  ]);
  routerActive = routerA;
  streamingOrigin = await serveSsrApp(() => streaming);
  islandsOrigin = await serveSsrApp(() => islands);
  contextOrigin = await serveSsrApp(() => context);
  routerOrigin = await serveSsrApp(() => routerActive);
  devOrigin = await startDevServer(fixture("streaming"));
  devRouterOrigin = await startDevServer(fixture("router"), [defineGlobals({ __RZ_TEST_VERSION__: "dev" })], {
    devContext: (request) => ({ tenant: request.headers.get("x-tenant") ?? undefined }),
  });
  origins.push(streamingOrigin, islandsOrigin, contextOrigin, routerOrigin, devOrigin, devRouterOrigin);
  for (const engine of ENGINES) browsers.set(engine, await launchEngine(engine));
}, 900_000);

afterAll(async () => {
  for (const origin of origins) await origin.close();
  for (const browser of browsers.values()) await browser.close();
  rmSync(OUT, { recursive: true, force: true });
});

function islandDescriptors(html: string): Array<Record<string, unknown>> {
  return [...html.matchAll(/<script type="application\/json" data-rz-island="[^"]*">([\s\S]*?)<\/script>/g)].map(
    (match) => JSON.parse(match[1] ?? "{}") as Record<string, unknown>,
  );
}

test(
  "server renders are isolated per request: beforeLoad context and concurrent module-level signals",
  async () => {
    const [first, second] = await Promise.all([
      routerA.handler(new Request("http://127.0.0.1/context?scope=request-one")).then((response) => response.text()),
      routerA.handler(new Request("http://127.0.0.1/context?scope=request-two")).then((response) => response.text()),
    ]);
    expect(first).toMatch(/id="?request-scope"?>none:request-one</);
    expect(second).toMatch(/id="?request-scope"?>none:request-two</);

    const [slow, fast] = await Promise.all([
      moduleState.handler(new Request("http://127.0.0.1/?d=200")).then((response) => response.text()),
      moduleState.handler(new Request("http://127.0.0.1/?d=10")).then((response) => response.text()),
    ]);
    expect(slow).toMatch(/<p id="?hits"?>1<\/p>/);
    expect(fast).toMatch(/<p id="?hits"?>1<\/p>/);
    expect(slow).toMatch(/<p id="?tone"?>loud<\/p>/);
  },
  TIMEOUT,
);

test(
  "the handler's context is the base that layout beforeLoad extends, and devContext supplies it in dev",
  async () => {
    const built = await routerA.handler(new Request("http://127.0.0.1/context?scope=built"), { context: { tenant: "acme" } });
    expect(await built.text()).toMatch(/id="?request-scope"?>acme:built</);
    const dev = await fetch(`${devRouterOrigin.origin}/context?scope=dev`, { headers: { accept: "text/html", "x-tenant": "globex" } });
    expect(await dev.text()).toMatch(/id="?request-scope"?>globex:dev</);
  },
  TIMEOUT,
);

test(
  "the shell arrives before pending work settles and patches follow",
  async () => {
    const chunks = await readChunks(await streaming.handler(new Request("http://127.0.0.1/")));
    const first = chunks[0] ?? "";
    expect(first).toContain("<!--rz:0-->");
    expect(first).toContain("loading slow");
    expect(first).toMatch(/loading slow[\s\S]*<\/div>/);
    expect(first).not.toContain("data-rz-patch");
    expect(chunks.some((chunk) => chunk.includes("<template data-rz-patch=") && chunk.includes("done"))).toBe(true);
    expect(first).not.toContain("content=settled");
    expect(
      chunks.some(
        (chunk) => chunk.includes('<template data-rz-head data-rz-head-key="meta:name:slow">') && chunk.includes("content=settled"),
      ),
    ).toBe(true);
    const endIndex = chunks.findIndex((chunk) => chunk.includes("data-rz-end"));
    expect(endIndex).toBeGreaterThan(0);
    expect(chunks.slice(endIndex).join("")).toMatch(/<\/body>\s*<\/html>\s*$/);
  },
  TIMEOUT,
);

test(
  "a render that fails before the first byte answers 500 inside the document",
  async () => {
    const response = await routerA.handler(new Request("http://127.0.0.1/broken"));
    expect(response.status).toBe(500);
    expect(response.headers.get("content-type")).toBe("text/html; charset=utf-8");
    const html = await response.text();
    expect(html).toMatch(/^<!doctype html>/i);
    expect(html).toMatch(/<title data-rz-head="">Router<\/title>/);
    expect(html).toContain("navigator.serviceWorker?.register");
    expect(html).toMatch(/<div id="?app"?>Internal Server Error<\/div>/);
    expect(html).not.toContain('type="module"');
    expect(html).not.toContain("broken preload");
  },
  TIMEOUT,
);

for (const engine of ENGINES) {
  test(
    `${engine}: islands name their static parent, escalate function props, run async loads on the server and keep typed values`,
    async () => {
      const html = await (await islands.handler(new Request("http://127.0.0.1/"))).text();
      expect(islandDescriptors(html).map((descriptor) => descriptor.e)).toContain("Parent");

      const escalated = await openPage(browsers.get(engine)!, `${islandsOrigin.origin}/`);
      try {
        await clickUntil(escalated.page, "#pick-b", "document.querySelector('#picked')?.textContent === 'b'");
        expect(escalated.errors).toEqual([]);
      } finally {
        await escalated.close();
      }

      const seeded = await openPage(browsers.get(engine)!, `${islandsOrigin.origin}/`);
      try {
        await clickUntil(seeded.page, "#seeded", "document.querySelector('#seeded')?.textContent === 'seeded value:1'");
        expect(await seeded.page.evaluate("globalThis.__loads ?? 0")).toBe(0);
        expect(seeded.errors).toEqual([]);
      } finally {
        await seeded.close();
      }

      const typed = await openPage(browsers.get(engine)!, `${islandsOrigin.origin}/`, {
        beforeHydration: async (held) => {
          await held.fill("#name", "typed text");
          await held.focus("#name");
          await held.evaluate("window.__before = document.querySelector('#name')");
        },
      });
      try {
        await waitFor(typed.page, "window.__before != null && document.querySelector('#name') !== window.__before");
        expect(await typed.page.inputValue("#name")).toBe("typed text");
        expect(await typed.page.evaluate("document.activeElement?.id")).toBe("name");
        expect(typed.errors).toEqual([]);
      } finally {
        await typed.close();
      }

      const formModule = String(islandDescriptors(html).find((descriptor) => descriptor.e === "Form")?.m);
      const earlyContext = await browsers.get(engine)!.newContext();
      const early = await earlyContext.newPage();
      const formRequested = Promise.withResolvers<void>();
      const formReleased = Promise.withResolvers<void>();
      await early.route(`**${formModule}`, async (route) => {
        formRequested.resolve();
        await formReleased.promise;
        await route.continue();
      });
      try {
        await early.goto(`${islandsOrigin.origin}/`, { waitUntil: "domcontentloaded" });
        await formRequested.promise;
        await early.click("#probe", { noWaitAfter: true });
        formReleased.resolve();
        await waitFor(early, "document.querySelector('#probe')?.textContent === '1'");
      } finally {
        formReleased.resolve();
        await earlyContext.close();
      }
    },
    TIMEOUT,
  );

  test(
    `${engine}: islands in the children of an island read the context its body provides, and the document links its own assets`,
    async () => {
      const response = await context.handler(new Request("http://127.0.0.1/"));
      expect(response.status).toBe(200);
      const html = await response.text();
      expect(html).toMatch(/<button id="?trigger"? type="?button"?>closed<\/button>/);
      expect(islandDescriptors(html).map((descriptor) => descriptor.e)).toEqual(["Root", "Trigger", "Content"]);
      const mark = /<link rel="?preload"? as="?image"? type="?image\/svg\+xml"? href="?([^"\s>]+)"?/.exec(html)?.[1];
      expect(mark).toMatch(/^\/assets\/mark-[^/]+\.svg$/);
      expect(existsSync(join(context.clientDir, mark!))).toBe(true);

      const { page, errors, close } = await openPage(browsers.get(engine)!, `${contextOrigin.origin}/`);
      try {
        await clickUntil(page, "#trigger", "document.querySelector('#trigger')?.textContent === 'open'");
        await waitFor(page, "document.querySelector('#content')?.textContent === 'shown'");
        expect(await page.evaluate("document.querySelectorAll('#trigger, #content').length")).toBe(2);
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    TIMEOUT,
  );

  test(
    `${engine}: patches reach the page, islands boot and static modules never load`,
    async () => {
      const { page, errors, requests, close } = await openPage(browsers.get(engine)!, `${streamingOrigin.origin}/`);
      try {
        await waitFor(page, "document.querySelector('#slow')?.textContent === 'done'");
        await waitFor(page, "document.head.querySelector('meta[name=\"slow\"]')?.getAttribute('content') === 'settled'");
        await clickUntil(page, "#counter", "document.querySelector('#counter')?.textContent === '1'");
        const staticModule = streaming.registry.modules["src/Static.tsx"];
        expect(staticModule).toBeDefined();
        expect(requests.some((url) => url.endsWith(staticModule!.url))).toBe(false);
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    TIMEOUT,
  );

  test(
    `${engine}: swap navigation keeps the page, and a build mismatch turns the next swap into a full load`,
    async () => {
      const swap = await openPage(browsers.get(engine)!, `${routerOrigin.origin}/`);
      try {
        await swap.page.evaluate("window.__marker = 'kept'");
        await swap.page.click("#nav-about");
        await waitFor(
          swap.page,
          "location.pathname === '/about' && document.title === 'About' && document.querySelector('#about') !== null",
        );
        expect(await swap.page.evaluate("window.__marker")).toBe("kept");
        expect(swap.errors).toEqual([]);
      } finally {
        await swap.close();
      }

      const mismatch = await openPage(browsers.get(engine)!, `${routerOrigin.origin}/`);
      try {
        await mismatch.page.evaluate("window.__marker = 'kept'");
        routerActive = routerB;
        await mismatch.page.click("#nav-news");
        await waitFor(
          mismatch.page,
          "location.pathname === '/news' && window.__marker === undefined && document.querySelector('#version')?.textContent === 'two'",
        );
        expect(mismatch.errors).toEqual([]);
      } finally {
        routerActive = routerA;
        await mismatch.close();
      }
    },
    TIMEOUT,
  );

  test.skipIf(engine !== "chromium")(
    `${engine}: service worker caches keep swaps and islands working offline`,
    async () => {
      const { page, errors, close } = await openPage(browsers.get(engine)!, `${routerOrigin.origin}/`);
      try {
        await waitFor(page, "navigator.serviceWorker.controller !== null");
        await page.reload();
        await waitFor(page, "navigator.serviceWorker.controller !== null");
        await clickUntil(page, "#counter", "document.querySelector('#counter')?.textContent === '1'");
        await page.click("#nav-about");
        await waitFor(page, "location.pathname === '/about' && document.querySelector('#about') !== null");
        await page.click("#nav-home");
        await waitFor(page, "location.pathname === '/' && document.querySelector('#counter') !== null");

        await page.context().setOffline(true);
        await page.reload();
        await clickUntil(page, "#counter", "document.querySelector('#counter')?.textContent === '1'");
        await page.evaluate("window.__marker = 'kept'");
        await page.click("#nav-about");
        await waitFor(page, "location.pathname === '/about' && document.querySelector('#about') !== null");
        expect(await page.evaluate("window.__marker")).toBe("kept");

        await page.click("#nav-news");
        await waitFor(page, "document.querySelector('#sw-offline') !== null");
        expect(page.url()).toBe(`${routerOrigin.origin}/news`);
        expect(errors).toEqual([]);
      } finally {
        await page.context().setOffline(false);
        await close();
      }
    },
    TIMEOUT,
  );

  test(
    `${engine}: a failing dev render keeps the document and shows the error overlay`,
    async () => {
      const response = await fetch(`${devRouterOrigin.origin}/broken`, { headers: { accept: "text/html" } });
      expect(response.status).toBe(500);
      expect(await response.text()).toMatch(/<title data-rz-head="">Router<\/title>/);
      const { page, close } = await openPage(browsers.get(engine)!, `${devRouterOrigin.origin}/broken`);
      try {
        await waitFor(page, "document.querySelector('vite-error-overlay')?.shadowRoot?.textContent.includes('broken preload') === true");
      } finally {
        await close();
      }
    },
    TIMEOUT,
  );

  test.skipIf(engine !== "chromium")(
    `${engine}: editing the document in dev reloads the page with the new shell`,
    async () => {
      const root = join(OUT, "dev-document");
      cpSync(fixture("streaming"), root, { recursive: true });
      symlinkSync(join(import.meta.dirname, "..", "node_modules"), join(root, "node_modules"), "dir");
      const origin = await startDevServer(root);
      const { page, close } = await openPage(browsers.get(engine)!, `${origin.origin}/`);
      try {
        await waitFor(page, "document.title === 'Streaming'");
        const file = join(root, "src", "document.tsx");
        writeFileSync(file, readFileSync(file, "utf8").replace("<title>Streaming</title>", "<title>Edited</title>"));
        await waitFor(page, "document.title === 'Edited' && document.querySelector('#slow')?.textContent === 'done'");
      } finally {
        await close();
        await origin.close();
      }
    },
    TIMEOUT,
  );

  test(
    `${engine}: the dev server streams the same SSR`,
    async () => {
      const html = await (await fetch(`${devOrigin.origin}/`, { headers: { accept: "text/html" } })).text();
      expect(html).toContain("data-rz-patch");
      const { page, errors, close } = await openPage(browsers.get(engine)!, `${devOrigin.origin}/`);
      try {
        await waitFor(page, "document.querySelector('#slow')?.textContent === 'done'");
        await clickUntil(page, "#counter", "document.querySelector('#counter')?.textContent === '1'");
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    TIMEOUT,
  );
}
