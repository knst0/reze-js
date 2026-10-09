import { mkdtempSync, rmSync } from "node:fs";
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
let routerA: SsrBuild;
let routerB: SsrBuild;
let routerActive: SsrBuild;
let streamingOrigin: SsrOrigin;
let islandsOrigin: SsrOrigin;
let routerOrigin: SsrOrigin;
let devOrigin: SsrOrigin;

function fixture(name: string): string {
  return join(FIXTURES, name);
}

beforeAll(async () => {
  [moduleState, streaming, islands, routerA, routerB] = await Promise.all([
    buildSsrFixture({ fixtureDir: fixture("module-state"), outDir: join(OUT, "module-state") }),
    buildSsrFixture({ fixtureDir: fixture("streaming"), outDir: join(OUT, "streaming") }),
    buildSsrFixture({ fixtureDir: fixture("islands"), outDir: join(OUT, "islands") }),
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
  routerOrigin = await serveSsrApp(() => routerActive);
  devOrigin = await startDevServer(fixture("streaming"));
  origins.push(streamingOrigin, islandsOrigin, routerOrigin, devOrigin);
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
  "concurrent requests see their own module-level signals",
  async () => {
    const [slow, fast] = await Promise.all([
      moduleState.handler(new Request("http://127.0.0.1/?d=200")).then((response) => response.text()),
      moduleState.handler(new Request("http://127.0.0.1/?d=10")).then((response) => response.text()),
    ]);
    expect(slow).toMatch(/<p id="?hits"?>1<\/p>/);
    expect(fast).toMatch(/<p id="?hits"?>1<\/p>/);
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
    const endIndex = chunks.findIndex((chunk) => chunk.includes("data-rz-end"));
    expect(endIndex).toBeGreaterThan(0);
    expect(chunks.slice(endIndex).join("")).toMatch(/<\/body>\s*<\/html>\s*$/);
  },
  TIMEOUT,
);

test(
  "the island descriptor for a static parent names the parent component",
  async () => {
    const html = await (await islands.handler(new Request("http://127.0.0.1/"))).text();
    expect(islandDescriptors(html).map((descriptor) => descriptor.e)).toContain("Parent");
  },
  TIMEOUT,
);

for (const engine of ENGINES) {
  test(
    `${engine}: patches reach the page, islands boot and static modules never load`,
    async () => {
      const { page, errors, requests, close } = await openPage(browsers.get(engine)!, `${streamingOrigin.origin}/`);
      try {
        await waitFor(page, "document.querySelector('#slow')?.textContent === 'done'");
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
    `${engine}: a static parent owning a function prop is escalated to the island root`,
    async () => {
      const { page, errors, close } = await openPage(browsers.get(engine)!, `${islandsOrigin.origin}/`);
      try {
        await clickUntil(page, "#pick-b", "document.querySelector('#picked')?.textContent === 'b'");
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    TIMEOUT,
  );

  test(
    `${engine}: async island loads run on the server only`,
    async () => {
      const { page, errors, close } = await openPage(browsers.get(engine)!, `${islandsOrigin.origin}/`);
      try {
        await clickUntil(page, "#seeded", "document.querySelector('#seeded')?.textContent === 'seeded value:1'");
        expect(await page.evaluate("globalThis.__loads ?? 0")).toBe(0);
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    TIMEOUT,
  );

  test(
    `${engine}: typed values and focus survive the island boot`,
    async () => {
      const { page, errors, close } = await openPage(browsers.get(engine)!, `${islandsOrigin.origin}/`, {
        beforeHydration: async (held) => {
          await held.fill("#name", "typed text");
          await held.focus("#name");
          await held.evaluate("window.__before = document.querySelector('#name')");
        },
      });
      try {
        await waitFor(page, "window.__before != null && document.querySelector('#name') !== window.__before");
        expect(await page.inputValue("#name")).toBe("typed text");
        expect(await page.evaluate("document.activeElement?.id")).toBe("name");
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    TIMEOUT,
  );

  test(
    `${engine}: swap navigation keeps the page and updates the url and title`,
    async () => {
      const { page, errors, close } = await openPage(browsers.get(engine)!, `${routerOrigin.origin}/`);
      try {
        await page.evaluate("window.__marker = 'kept'");
        await page.click("#nav-about");
        await waitFor(page, "location.pathname === '/about' && document.title === 'About' && document.querySelector('#about') !== null");
        expect(await page.evaluate("window.__marker")).toBe("kept");
        expect(errors).toEqual([]);
      } finally {
        await close();
      }
    },
    TIMEOUT,
  );

  test(
    `${engine}: a build mismatch turns a swap into a full load`,
    async () => {
      const { page, errors, close } = await openPage(browsers.get(engine)!, `${routerOrigin.origin}/`);
      try {
        await page.evaluate("window.__marker = 'kept'");
        routerActive = routerB;
        await page.click("#nav-news");
        await waitFor(
          page,
          "location.pathname === '/news' && window.__marker === undefined && document.querySelector('#version')?.textContent === 'two'",
        );
        expect(errors).toEqual([]);
      } finally {
        routerActive = routerA;
        await close();
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
