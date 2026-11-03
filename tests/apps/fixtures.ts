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
export type ServerKind = "js" | "js-build" | "rust" | "rust-actix" | "spa";

export interface Server {
  url(path: string): string;
  /** With `rust`: the same build on the JS server, whose external loaders run in the Rust server. */
  js?(path: string): string;
}

interface Running {
  origin: string;
  js?: string;
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

/** Builds `app` into a temporary directory, with the Rust types of `rust`. */
async function build(app: string, rust?: string[]): Promise<{ out: string; client: string; handler: (options?: object) => Handler }> {
  const out = await mkdtemp(join(tmpdir(), "rz-build-"));
  await writeFile(join(out, "package.json"), '{ "type": "module" }');
  const client = join(out, "client");
  const builder = await createBuilder({
    root: app,
    configFile: false,
    logLevel: "warn",
    plugins: [reze({ ...optionsOf(app), ...(rust ? { rust: { types: rust } } : {}) })],
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

function run(command: string, args: string[], env: Record<string, string> = {}): Promise<void> {
  return new Promise((done, fail) => {
    const child = spawn(command, args, { cwd: repo, env: { ...process.env, ...env }, stdio: ["ignore", "ignore", "pipe"] });
    let stderr = "";
    child.stderr.on("data", (chunk: Buffer) => (stderr += chunk));
    child.on("error", fail);
    child.on("exit", (code) => (code === 0 ? done() : fail(new Error(`${command} ${args.join(" ")} failed:\n${stderr}`))));
  });
}

/** Builds the app's `server` crate, whose `types` crate holds the loader data types, and serves the build with it. */
async function startRust(app: string, adapter: "tower" | "actix"): Promise<Running> {
  const name = `${app.split("/").at(-1)}_server`;
  const target = join(repo, "target/e2e");
  const ir = join(await mkdtemp(join(tmpdir(), "rz-ir-")), "types.json");
  await run("cargo", ["run", "-q", "-p", "reze_build", "--bin", "reze-ir", "--", join(app, "types"), ir]);
  const { out, client, handler } = await build(app, [ir]);
  await run("cargo", ["build", "-q", "--manifest-path", join(app, "Cargo.toml"), "-p", name], { CARGO_TARGET_DIR: target });
  const child = spawn(join(target, "debug", name), [join(out, "server/reze.json"), adapter], { stdio: ["ignore", "pipe", "inherit"] });
  const rust = await new Promise<string>((done, fail) => {
    child.stdout.once("data", (chunk: Buffer) => done(chunk.toString().trim()));
    child.on("exit", (code) => fail(new Error(`${name} exited with ${code}`)));
  });
  const proxy: Handler = (request) => {
    const url = new URL(request.url);
    const headers = new Headers(request.headers);
    for (const name of ["host", "connection", "keep-alive", "content-length", "transfer-encoding"]) headers.delete(name);
    return fetch(new URL(url.pathname + url.search, rust), { method: request.method, headers, redirect: "manual" });
  };
  const js = await serveBuild(client, handler({ loaders: rust }), async () => {});
  const running = await serveBuild(client, proxy, async () => {
    child.kill();
    await js.close();
    await rm(out, { recursive: true, force: true });
    await rm(dirname(ir), { recursive: true, force: true });
  });
  return { ...running, js: js.origin };
}

function start(kind: ServerKind, app: string): Promise<Running> {
  if (kind === "rust" || kind === "rust-actix") return startRust(app, kind === "rust" ? "tower" : "actix");
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
    const { origin, js } = await running;
    await use({ url: (path) => new URL(path, origin).href, js: js ? (path) => new URL(path, js).href : undefined });
  },
});
