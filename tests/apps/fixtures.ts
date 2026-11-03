import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { dirname, extname, join, normalize } from "node:path";
import { Readable } from "node:stream";
import { pathToFileURL } from "node:url";

import { test as base } from "@playwright/test";
import { createBuilder, createServer as createVite } from "vite";

import reze from "../../packages/vite-plugin/src/index";
import { sourceAliases } from "../../vitest.shared";

export { expect } from "@playwright/test";

/** `js` runs the Vite dev server; `js-build` serves a production build. */
export type ServerKind = "js" | "js-build" | "rust";

export interface Server {
  url(path: string): string;
}

interface Running {
  origin: string;
  close(): Promise<void>;
}

type Handler = (request: Request) => Promise<Response>;

function toRequest(message: IncomingMessage, origin: string): Request {
  const headers = new Headers();
  for (const [name, value] of Object.entries(message.headers)) {
    for (const item of Array.isArray(value) ? value : value ? [value] : []) {
      headers.append(name, item);
    }
  }
  const body = message.method === "GET" || message.method === "HEAD" ? undefined : (Readable.toWeb(message) as ReadableStream);
  return new Request(new URL(message.url ?? "/", origin), {
    method: message.method,
    headers,
    body,
    duplex: "half",
  } as RequestInit);
}

async function send(response: Response, out: ServerResponse): Promise<void> {
  const headers: Record<string, string | string[]> = {};
  response.headers.forEach((value, name) => {
    if (name !== "set-cookie") headers[name] = value;
  });
  const cookies = response.headers.getSetCookie();
  if (cookies.length > 0) headers["set-cookie"] = cookies;
  out.writeHead(response.status, headers);
  if (!response.body) return void out.end();
  for await (const chunk of response.body as unknown as AsyncIterable<Uint8Array>) out.write(chunk);
  out.end();
}

async function startJs(app: string): Promise<Running> {
  const vite = await createVite({
    root: app,
    configFile: false,
    logLevel: "warn",
    appType: "custom",
    plugins: [reze()],
    resolve: { alias: sourceAliases },
    server: { middlewareMode: true, ws: false, watch: null },
  });
  const entry = join(app, "src/server.ts");
  const handler: Handler = existsSync(entry)
    ? (await vite.ssrLoadModule("/src/server.ts")).default
    : (await vite.ssrLoadModule("reze-js/server")).createHandler();
  return listen(
    (message, out) => vite.middlewares(message, out, () => respond(handler, message, out)),
    () => vite.close(),
  );
}

async function listen(serve: (message: IncomingMessage, out: ServerResponse) => void, close: () => Promise<void>): Promise<Running> {
  const http = createServer(serve);
  await new Promise<void>((resolve) => http.listen(0, "127.0.0.1", resolve));
  const { port } = http.address() as AddressInfo;
  return {
    origin: `http://127.0.0.1:${port}`,
    async close() {
      await new Promise((resolve) => http.close(resolve));
      await close();
    },
  };
}

function respond(handler: Handler, message: IncomingMessage, out: ServerResponse): void {
  handler(toRequest(message, `http://${message.headers.host}`))
    .then((response) => send(response, out))
    .catch((error: unknown) => {
      out.statusCode = 500;
      out.end(String(error));
    });
}

const types: Record<string, string> = { ".js": "text/javascript", ".css": "text/css" };

async function startJsBuild(app: string): Promise<Running> {
  const out = await mkdtemp(join(tmpdir(), "rz-build-"));
  await writeFile(join(out, "package.json"), '{ "type": "module" }');
  const client = join(out, "client");
  const builder = await createBuilder({
    root: app,
    configFile: false,
    logLevel: "warn",
    plugins: [reze()],
    resolve: { alias: sourceAliases },
    environments: {
      client: { build: { outDir: client, emptyOutDir: true } },
      ssr: { build: { outDir: join(out, "server"), emptyOutDir: true } },
    },
  });
  try {
    await builder.buildApp();
  } catch (error) {
    await rm(out, { recursive: true, force: true });
    throw error;
  }
  const entry = ["server.js", "server.mjs"].map((name) => join(out, "server", name)).find((path) => existsSync(path))!;
  const server = await import(pathToFileURL(entry).href);
  const handler: Handler = existsSync(join(app, "src/server.ts")) ? server.default : server.createHandler();
  return listen(
    (message, response) => {
      const path = normalize(new URL(message.url ?? "/", "http://localhost").pathname);
      readFile(join(client, path))
        .then((body) => {
          response.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" });
          response.end(body);
        })
        .catch(() => respond(handler, message, response));
    },
    () => rm(out, { recursive: true, force: true }),
  );
}

function start(kind: ServerKind, app: string): Promise<Running> {
  if (kind === "rust") throw new Error("The Rust server is not built yet");
  return kind === "js-build" ? startJsBuild(app) : startJs(app);
}

/** `server` runs the application whose directory holds the test file. */
export const test = base.extend<{ server: Server }, { serverKind: ServerKind; servers: Map<string, Promise<Running>> }>({
  serverKind: ["js", { option: true, scope: "worker" }],
  servers: [
    // oxlint-disable-next-line no-empty-pattern
    async ({}, use) => {
      const servers = new Map<string, Promise<Running>>();
      await use(servers);
      await Promise.all([...servers.values()].map(async (running) => (await running).close()));
    },
    { scope: "worker" },
  ],
  server: async ({ servers, serverKind }, use, testInfo) => {
    const app = dirname(testInfo.file);
    let running = servers.get(app);
    if (!running) servers.set(app, (running = start(serverKind, app)));
    const { origin } = await running;
    await use({ url: (path) => new URL(path, origin).href });
  },
});
