import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { createServer } from "node:http";
import type { Server } from "node:http";
import { extname, join, relative, sep } from "node:path";
import { createBuilder, version as viteVersion } from "vite";
import type { Plugin } from "vite";
import { chromium, firefox, webkit } from "playwright";
import type { Browser, Page } from "playwright";

export type SsgBrowserName = "chromium" | "firefox" | "webkit";

const Engines = {
  chromium,
  firefox,
  webkit,
} as const;

export function resolveBrowsers(raw: string | undefined = process.env.REZE_SSG_BROWSERS): SsgBrowserName[] {
  if (raw === undefined || raw.trim() === "") return ["chromium"];
  const names = raw.split(",").map((part) => part.trim().toLowerCase()).filter((part) => part.length > 0);
  const out: SsgBrowserName[] = [];
  for (const name of names) {
    if (name !== "chromium" && name !== "firefox" && name !== "webkit") {
      throw new Error(`[reze-test] REZE_SSG_BROWSERS: unknown engine ${JSON.stringify(name)}, expected chromium, firefox or webkit`);
    }
    if (!out.includes(name)) out.push(name);
  }
  if (out.length === 0) return ["chromium"];
  return out;
}

export interface BuildSsgFixtureOptions {
  fixtureDir: string;
  outDir: string;
  plugins: Plugin | Plugin[];
  base?: string;
  htmlInput?: string;
}

export interface BuiltFixture {
  distDir: string;
  viteVersion: string;
}

export async function buildSsgFixture(options: BuildSsgFixtureOptions): Promise<BuiltFixture> {
  const builder = await createBuilder({
    root: options.fixtureDir,
    base: options.base ?? "/",
    configFile: false,
    logLevel: "warn",
    build: {
      outDir: options.outDir,
      emptyOutDir: true,
      manifest: true,
      modulePreload: { polyfill: false },
      rollupOptions: options.htmlInput === undefined ? undefined : { input: join(options.fixtureDir, options.htmlInput) },
    },
    plugins: Array.isArray(options.plugins) ? options.plugins : [options.plugins],
  });
  await builder.buildApp();
  return { distDir: options.outDir, viteVersion };
}

export async function expectBuildFails(plugins: Plugin | Plugin[], fixtureDir: string, outDir: string, pattern: RegExp): Promise<string> {
  let message = "";
  try {
    const builder = await createBuilder({
      root: fixtureDir,
      configFile: false,
      logLevel: "silent",
      build: { outDir, emptyOutDir: true, manifest: false, modulePreload: { polyfill: false } },
      plugins: Array.isArray(plugins) ? plugins : [plugins],
    });
    await builder.buildApp();
  } catch (error) {
    if (error instanceof Error) message = `${error.message}\n${error.stack ?? ""}`;
    else message = String(error);
  }
  if (message === "") throw new Error("[reze-test] expected the build to fail, but it succeeded");
  if (!pattern.test(message)) throw new Error(`[reze-test] build failed without the expected context ${String(pattern)}:\n${message.slice(0, 4000)}`);
  return message;
}

const MimeTypes: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".map": "application/json; charset=utf-8",
};

export interface StaticOrigin {
  origin: string;
  close: () => Promise<void>;
}

export function serveDist(distDir: string): Promise<StaticOrigin> {
  const absolute = distDir;
  const server: Server = createServer((req, res) => {
    const method = req.method ?? "GET";
    if (method !== "GET" && method !== "HEAD") {
      res.writeHead(405, { "content-type": "text/plain" });
      res.end("method not allowed");
      return;
    }
    const url = new URL(req.url ?? "/", "http://127.0.0.1");
    let pathname: string;
    try {
      pathname = decodeURIComponent(url.pathname);
    } catch {
      res.writeHead(400, { "content-type": "text/plain" });
      res.end("bad path");
      return;
    }
    if (pathname.includes("\0") || pathname.split("/").includes("..")) {
      res.writeHead(400, { "content-type": "text/plain" });
      res.end("bad path");
      return;
    }
    const candidates: string[] = [];
    if (pathname === "/") {
      candidates.push(join(absolute, "index.html"));
    } else if (pathname.endsWith("/")) {
      candidates.push(join(absolute, pathname.slice(1), "index.html"));
    } else {
      const direct = join(absolute, pathname.slice(1));
      candidates.push(direct);
      candidates.push(join(absolute, pathname.slice(1), "index.html"));
    }
    for (const file of candidates) {
      const relativePath = relative(absolute, file);
      if (relativePath.startsWith("..") || relativePath.includes(`..${sep}`)) continue;
      if (existsSync(file) && statSync(file).isFile()) {
        const body = readFileSync(file);
        res.writeHead(200, { "content-type": MimeTypes[extname(file).toLowerCase()] ?? "application/octet-stream" });
        res.end(method === "HEAD" ? undefined : body);
        return;
      }
    }
    res.writeHead(404, { "content-type": "text/plain" });
    res.end("not found");
  });
  const gate = Promise.withResolvers<StaticOrigin>();
  server.listen(0, "127.0.0.1", () => {
    const address = server.address();
    const port = typeof address === "object" && address !== null ? address.port : 0;
    const done = Promise.withResolvers<void>();
    gate.resolve({
      origin: `http://127.0.0.1:${port}`,
      close: () => {
        server.close((error) => {
          if (error) done.reject(error);
          else done.resolve();
        });
        return done.promise;
      },
    });
  });
  return gate.promise;
}

export async function fetchHtml(origin: string, pathname: string): Promise<{ status: number; html: string }> {
  const response = await fetch(`${origin}${pathname}`);
  return { status: response.status, html: await response.text() };
}

export function readBuiltFile(distDir: string, file: string): string {
  return readFileSync(join(distDir, file), "utf8");
}

export function listBuiltFiles(distDir: string, prefix = ""): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(join(distDir, prefix))) {
    const rel = prefix === "" ? entry : `${prefix}/${entry}`;
    if (statSync(join(distDir, rel)).isDirectory()) out.push(...listBuiltFiles(distDir, rel));
    else out.push(rel);
  }
  return out.sort();
}

export interface ViteManifestChunk {
  file: string;
  src?: string;
  imports?: string[];
  dynamicImports?: string[];
  css?: string[];
  assets?: string[];
  isEntry?: boolean;
}

export type ViteManifest = Record<string, ViteManifestChunk>;

export function readManifest(distDir: string): ViteManifest {
  const raw: unknown = JSON.parse(readBuiltFile(distDir, ".vite/manifest.json"));
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    throw new Error("[reze-test] vite manifest is not a chunk record");
  }
  return raw as ViteManifest;
}

export function manifestClosure(manifest: ViteManifest, roots: readonly string[]): Set<string> {
  const byFile = new Map(Object.values(manifest).map((chunk) => [chunk.file, chunk]));
  const seen = new Set<string>();
  const queue = [...roots];
  while (queue.length > 0) {
    const file = queue.pop();
    if (file === undefined || seen.has(file)) continue;
    seen.add(file);
    const chunk = byFile.get(file);
    if (chunk === undefined) continue;
    queue.push(...(chunk.imports ?? []), ...(chunk.dynamicImports ?? []));
  }
  return seen;
}

export function payloadScript(html: string, rootId: string): string {
  const marker = `data-reze-state="${rootId}"`;
  const scriptIndex = html.indexOf(marker);
  if (scriptIndex < 0) throw new Error(`[reze-test] built page has no hydration payload for root ${JSON.stringify(rootId)}`);
  const openEnd = html.indexOf(">", scriptIndex);
  const closeStart = html.indexOf("</script>", openEnd);
  if (openEnd < 0 || closeStart < 0) throw new Error("[reze-test] hydration payload script is truncated");
  return html.slice(openEnd + 1, closeStart);
}

export interface OpenPageResult {
  page: Page;
  errors: unknown[];
  requests: string[];
  close: () => Promise<void>;
}

export async function launchEngine(name: SsgBrowserName): Promise<Browser> {
  try {
    return await Engines[name].launch();
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(`[reze-test] cannot launch ${name}: ${detail}. Install it with "pnpm exec playwright install --with-deps ${name}".`);
  }
}

export async function openPage(browser: Browser, url: string, options?: { javaScript?: boolean; beforeHydration?: (page: Page) => Promise<void> }): Promise<OpenPageResult> {
  const context = await browser.newContext({ javaScriptEnabled: options?.javaScript ?? true });
  const page = await context.newPage();
  const errors: unknown[] = [];
  const requests: string[] = [];
  page.on("pageerror", (error) => errors.push(error));
  page.on("request", (request) => requests.push(request.url()));
  if (options?.beforeHydration !== undefined) {
    const bootstrap = Promise.withResolvers<void>();
    await page.route("**/*.js", async route => {
      await bootstrap.promise;
      await route.continue();
    });
    try {
      await page.goto(url, { waitUntil: "commit" });
      await options.beforeHydration(page);
    } finally {
      bootstrap.resolve();
    }
    await page.waitForLoadState("load");
    await page.unroute("**/*.js");
  } else {
    await page.goto(url, { waitUntil: "load" });
  }
  return {
    page,
    errors,
    requests,
    close: () => context.close(),
  };
}

export function waitFor(page: Page, predicate: string, timeoutMs = 15_000): Promise<void> {
  return page.waitForFunction(predicate, undefined, { timeout: timeoutMs }).then(() => undefined);
}
