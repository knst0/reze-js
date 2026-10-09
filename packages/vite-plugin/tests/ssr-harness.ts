import { existsSync, readFileSync, statSync } from "node:fs";
import { createServer } from "node:http";
import type { Server } from "node:http";
import { extname, join, relative } from "node:path";
import { pathToFileURL } from "node:url";

import type { Page } from "playwright";
import { createNodeListener } from "reze-js/node";
import { createServer as createDevServer } from "vite";
import type { Plugin } from "vite";

import reze from "../src/index";
import { buildSsgFixture } from "./ssg-harness";

export interface SsrRegistryEntry {
  url: string;
  css: readonly string[];
  preload: readonly string[];
}

export interface SsrRegistry {
  buildId: string;
  base: string;
  rootId: string;
  modules: Record<string, SsrRegistryEntry>;
}

export interface SsrBuild {
  clientDir: string;
  handler: (request: Request) => Promise<Response>;
  registry: SsrRegistry;
}

export interface SsrOrigin {
  origin: string;
  close: () => Promise<void>;
}

export interface SsrFixtureOptions {
  fixtureDir: string;
  outDir: string;
  plugins?: Plugin[];
}

export async function buildSsrFixture(options: SsrFixtureOptions): Promise<SsrBuild> {
  const serverDir = `${options.outDir}-server`;
  await buildSsgFixture({
    fixtureDir: options.fixtureDir,
    outDir: options.outDir,
    plugins: [...reze({ ssr: { entry: "src/app.tsx", outDir: serverDir } }), ...(options.plugins ?? [])],
  });
  const server = (await import(pathToFileURL(join(serverDir, "entry.js")).href)) as {
    handler: SsrBuild["handler"];
  };
  const client = (await import(pathToFileURL(join(serverDir, "reze-client.js")).href)) as {
    default: SsrRegistry;
  };
  return { clientDir: options.outDir, handler: server.handler, registry: client.default };
}

export function defineGlobals(values: Record<string, string>): Plugin {
  return {
    name: "rz-test-define",
    config: () => ({
      define: Object.fromEntries(Object.entries(values).map(([name, value]) => [name, JSON.stringify(value)])),
    }),
  };
}

const MIME: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
};

function staticFile(root: string, pathname: string): string | undefined {
  if (pathname === "/") return undefined;
  const file = join(root, decodeURIComponent(pathname));
  const path = relative(root, file);
  if (path === "" || path.startsWith("..")) return undefined;
  return existsSync(file) && statSync(file).isFile() ? file : undefined;
}

export function serveSsrApp(select: () => SsrBuild): Promise<SsrOrigin> {
  const server: Server = createServer((req, res) => {
    const build = select();
    const { pathname } = new URL(req.url ?? "/", "http://127.0.0.1");
    const file = staticFile(build.clientDir, pathname);
    if (file === undefined) {
      createNodeListener(build.handler)(req, res);
      return;
    }
    res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
    res.end(readFileSync(file));
  });
  return listen(server);
}

export async function startDevServer(fixtureDir: string): Promise<SsrOrigin> {
  const server = await createDevServer({
    root: fixtureDir,
    configFile: false,
    logLevel: "silent",
    plugins: [...reze({ ssr: { entry: "src/app.tsx" } })],
    server: { host: "127.0.0.1", port: 0 },
  });
  await server.listen();
  const local = server.resolvedUrls?.local[0];
  if (local === undefined) throw new Error("[reze] dev server did not report a local url");
  return { origin: local.replace(/\/$/, ""), close: () => server.close() };
}

function listen(server: Server): Promise<SsrOrigin> {
  const { promise, resolve, reject } = Promise.withResolvers<SsrOrigin>();
  server.once("error", reject);
  server.listen(0, "127.0.0.1", () => {
    const address = server.address();
    const port = typeof address === "object" && address !== null ? address.port : 0;
    resolve({
      origin: `http://127.0.0.1:${port}`,
      close: () =>
        new Promise<void>((done, fail) => {
          server.close((error) => (error ? fail(error) : done()));
        }),
    });
  });
  return promise;
}

export async function readChunks(response: Response): Promise<string[]> {
  if (response.body === null) throw new Error("[reze] response has no body");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const chunks: string[] = [];
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(decoder.decode(value, { stream: true }));
  }
  const tail = decoder.decode();
  if (tail !== "") chunks.push(tail);
  return chunks;
}

export async function clickUntil(page: Page, selector: string, predicate: string, timeoutMs = 20_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    await page.click(selector, { timeout: 500, noWaitAfter: true }).catch(() => undefined);
    if (await page.evaluate(predicate)) return;
    if (Date.now() >= deadline) throw new Error(`[reze] ${predicate} never held after clicking ${selector}`);
    await page.waitForTimeout(100);
  }
}
