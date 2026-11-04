import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { dirname, extname, join, normalize, resolve } from "node:path";
import { Readable } from "node:stream";
import { pathToFileURL } from "node:url";

import { test as base } from "@playwright/test";
import { createBuilder, createServer as createVite } from "vite";

import reze from "../../packages/vite-plugin/src/index";
import { sourceAliases } from "../../vitest.shared";

export { expect } from "@playwright/test";

/**
 * `js` runs the Vite dev server; `js-build` serves a production build; `rust` serves it from the Rust server; `spa` serves
 * the static build of `target: "spa"`.
 */
export type ServerKind = "js" | "js-build" | "rust" | "spa";

export interface Server {
  url(path: string): string;
  /** With `rust`: the same build on the JS server, whose external loaders run in the Rust server. */
  js?(path: string): string;
  /** With `rust`: runs `reze check --render-diff` over `paths` and resolves to its exit code and output. */
  renderDiff?(paths: string[]): Promise<{ code: number; output: string }>;
}

interface Running {
  origin: string;
  js?: string;
  rust?: { origin: string; build: string };
  close(): Promise<void>;
}

type Handler = (request: Request, hints?: (links: string[]) => void) => Promise<Response>;

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

/** The plugin options of `app`, from its `reze.json`. */
function optionsOf(app: string): Parameters<typeof reze>[0] {
  const file = join(app, "reze.json");
  return existsSync(file) ? JSON.parse(readFileSync(file, "utf8")) : {};
}

async function startJs(app: string): Promise<Running> {
  const vite = await createVite({
    root: app,
    configFile: false,
    logLevel: "warn",
    appType: "custom",
    plugins: [reze(optionsOf(app))],
    resolve: { alias: sourceAliases },
    server: { middlewareMode: true, ws: false, watch: null },
  });
  return listen(
    (message, out) => vite.middlewares(message, out),
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
  handler(toRequest(message, `http://${message.headers.host}`), (links) => out.writeEarlyHints({ link: links }))
    .then((response) => send(response, out))
    .catch((error: unknown) => {
      out.statusCode = 500;
      out.end(String(error));
    });
}

const types: Record<string, string> = { ".js": "text/javascript", ".css": "text/css" };

/** Builds `app` into a temporary directory, with the cargo `features` of its Rust server. */
async function build(app: string, features?: string[]): Promise<{ out: string; client: string; handler: (options?: object) => Handler }> {
  const out = await mkdtemp(join(tmpdir(), "rz-build-"));
  await writeFile(join(out, "package.json"), '{ "type": "module" }');
  const client = join(out, "client");
  const builder = await createBuilder({
    root: app,
    configFile: false,
    logLevel: "warn",
    plugins: [reze({ ...optionsOf(app), ...(features ? { rust: { features } } : {}) })],
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
  const handler = (options?: object): Handler => (existsSync(join(app, "src/server.ts")) ? server.default : server.createHandler(options));
  return { out, client, handler };
}

/** Serves the files of `client`, and every other request with `handler`. */
function serveBuild(client: string, handler: Handler, close: () => Promise<void>): Promise<Running> {
  return listen((message, response) => {
    const url = new URL(message.url ?? "/", "http://localhost");
    const path = normalize(url.pathname);
    const file = extname(path) ? path : url.search ? "" : join(path, "index.html");
    if (!file || (message.method !== "GET" && message.method !== "HEAD")) return respond(handler, message, response);
    readFile(join(client, file))
      .then((body) => {
        response.writeHead(200, { "content-type": types[extname(file)] ?? "text/html; charset=utf-8" });
        response.end(body);
      })
      .catch(() => respond(handler, message, response));
  }, close);
}

async function startJsBuild(app: string): Promise<Running> {
  const { out, client, handler } = await build(app);
  return serveBuild(client, handler(), () => rm(out, { recursive: true, force: true }));
}

/** Builds `app` with `target: "spa"` and serves its files, with `index.html` for every other path. */
async function startSpa(app: string): Promise<Running> {
  const out = await mkdtemp(join(tmpdir(), "rz-spa-"));
  const builder = await createBuilder({
    root: app,
    configFile: false,
    logLevel: "warn",
    plugins: [reze({ ...optionsOf(app), target: "spa" })],
    resolve: { alias: sourceAliases },
    environments: { client: { build: { outDir: out, emptyOutDir: true } } },
  });
  await builder.buildApp();
  return listen(
    (message, response) => {
      const path = normalize(new URL(message.url ?? "/", "http://localhost").pathname);
      const file = extname(path) ? path : "index.html";
      readFile(join(out, file)).then(
        (body) => {
          response.writeHead(200, { "content-type": types[extname(file)] ?? "text/html; charset=utf-8" });
          response.end(body);
        },
        () => {
          response.statusCode = 404;
          response.end();
        },
      );
    },
    () => rm(out, { recursive: true, force: true }),
  );
}

const repo = resolve(import.meta.dirname, "../..");

/** Builds the app with its Rust server, whose `dev` feature also runs the loaders of the JS server for the render diff, and serves the build with it. */
async function startRust(app: string): Promise<Running> {
  const { out, client, handler } = await build(app, ["reze_server/dev"]);
  const binary = join(out, "server", app.split("/").at(-1)!);
  const child = spawn(binary, [], { env: { ...process.env, HOST: "127.0.0.1", PORT: "0" }, stdio: ["ignore", "pipe", "inherit"] });
  const rust = await new Promise<string>((done, fail) => {
    child.stdout.on("data", (chunk: Buffer) => {
      const ready = /reze: listening on (\S+)/.exec(chunk.toString());
      if (ready) done(ready[1]!);
    });
    child.on("exit", (code) => fail(new Error(`${binary} exited with ${code}`)));
  });
  const js = await serveBuild(client, handler({ loaders: rust }), async () => {});
  return {
    origin: rust,
    js: js.origin,
    rust: { origin: rust, build: out },
    async close() {
      child.kill();
      await js.close();
      await rm(out, { recursive: true, force: true });
    },
  };
}

function check(rust: string, build: string, paths: string[]): Promise<{ code: number; output: string }> {
  const cli = join(repo, "packages/vite-plugin/dist/cli.js");
  return new Promise((done, fail) => {
    const child = spawn(process.execPath, [cli, "check", "--render-diff", "--rust", rust, "--build", build, ...paths], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "";
    child.stdout.on("data", (chunk: Buffer) => (output += chunk));
    child.stderr.on("data", (chunk: Buffer) => (output += chunk));
    child.on("error", fail);
    child.on("exit", (code) => done({ code: code ?? 1, output }));
  });
}

function start(kind: ServerKind, app: string): Promise<Running> {
  if (kind === "rust") return startRust(app);
  if (kind === "spa") return startSpa(app);
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
    if (!running) {
      if (serverKind.startsWith("rust")) testInfo.setTimeout(testInfo.timeout + 600_000);
      servers.set(app, (running = start(serverKind, app)));
    }
    const { origin, js, rust } = await running;
    await use({
      url: (path) => new URL(path, origin).href,
      js: js ? (path) => new URL(path, js).href : undefined,
      renderDiff: rust ? (paths) => check(rust.origin, rust.build, paths) : undefined,
    });
  },
});
