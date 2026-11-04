import { type ChildProcess, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import type { ServerResponse } from "node:http";
import { basename, dirname, extname, join, normalize, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { type Chunks, compileProjectAsync, type ProjectResult } from "@rezejs/compiler";
import { defaultClientConditions, defaultServerConditions, minify, type Plugin, transformWithOxc } from "vite";

import { type Handler, middleware } from "./node.ts";

export interface RezeOptions {
  /** Directory of route modules, relative to the Vite root. */
  routesDir?: string;
  chunks?: Chunks;
  /** `"spa"` builds a client-only application: a static `index.html` that renders every route in the browser. */
  target?: "server" | "spa";
  /** The Rust server, on when the Vite root has a `Cargo.toml`: `features` are the cargo features of its build and of the dev worker. */
  rust?: false | { features?: string[] };
}

const app = "virtual:rz-app";
const resolvedApp = `\0${app}`;
const dispatcher = "virtual:rz-dispatcher";
const stream = "virtual:rz-stream";
const route = "virtual:rz-route/";
const routes = "virtual:rz-routes";
const nav = "virtual:rz-nav";
const manifest = "virtual:rz-chunks";
const prerender = "virtual:rz-prerender";
const spaEntry = "virtual:rz-spa";
const rustTypesPrefix = "virtual:reze-types/";
/** The query of a route module's server render inside the client build of a client-only application. */
const serverQuery = "?rz-server";

/** The URL the dev server serves a virtual module at. */
const devUrl = (id: string) => `/@id/__x00__${id}`;

/** The handler of a server module: the default export of `src/server.ts`, or `createHandler()` of `reze-js/server`. */
const handlerOf = (module: Record<string, unknown>): Handler =>
  "createHandler" in module ? (module.createHandler as () => Handler)() : (module.default as Handler);

const types: Record<string, string> = {
  ".js": "text/javascript",
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".json": "application/json",
};

/** URLs of the client build, which the server build reads. */
interface Built {
  dispatcher: string;
  /** The module the dispatcher imports at start: the chunk URLs, the idle preload, and the handling of links and server-tier forms. */
  modules: string;
  stream: string;
  /** The index of each client chunk in the manifest that the dispatcher imports, by its key. */
  chunks: Record<string, number>;
}

/** Writes `source` to `file` unless it already holds it, and tells whether it changed. */
async function write(file: string, source: string): Promise<boolean> {
  if ((await readFile(file, "utf8").catch(() => undefined)) === source) return false;
  await mkdir(dirname(file), { recursive: true });
  await writeFile(file, source);
  return true;
}

/** Runs `cargo build --release` in `root` and returns the path of the root package's binary. */
function cargoBuild(root: string, features: string[]): Promise<string> {
  return new Promise((done, fail) => {
    const args = ["build", "--release", "--message-format=json-render-diagnostics"];
    if (features.length > 0) args.push("--features", features.join(","));
    const child = spawn("cargo", args, { cwd: root, stdio: ["ignore", "pipe", "inherit"] });
    const manifest = join(root, "Cargo.toml");
    let binary: string | undefined;
    let rest = "";
    child.stdout.on("data", (chunk: Buffer) => {
      const lines = (rest + chunk.toString()).split("\n");
      rest = lines.pop()!;
      for (const line of lines) {
        const message = JSON.parse(line) as { reason: string; manifest_path?: string; executable?: string | null };
        if (message.reason === "compiler-artifact" && message.executable && message.manifest_path === manifest) binary = message.executable;
      }
    });
    child.on("error", fail);
    child.on("exit", (code) => {
      if (code !== 0) fail(new Error(`cargo build failed with ${code}`));
      else if (!binary) fail(new Error(`${manifest} builds no binary: add \`[[bin]]\` with \`path = ".reze/main.rs"\``));
      else done(binary);
    });
  });
}

/** The Rust dev worker: `cargo run` with the `dev` feature of `reze_server`, on a free port. */
function startWorker(root: string, features: string[]): { child: ChildProcess; origin: Promise<string> } {
  const args = ["run", "--features", ["reze_server/dev", ...features].join(",")];
  const child = spawn("cargo", args, {
    cwd: root,
    env: { ...process.env, HOST: "127.0.0.1", PORT: "0" },
    stdio: ["ignore", "pipe", "inherit"],
  });
  const origin = new Promise<string>((done, fail) => {
    let rest = "";
    child.stdout!.on("data", (chunk: Buffer) => {
      const lines = (rest + chunk.toString()).split("\n");
      rest = lines.pop()!;
      for (const line of lines) {
        const ready = /^reze: listening on (\S+)/.exec(line);
        if (ready) done(ready[1]!);
        else process.stdout.write(`${line}\n`);
      }
    });
    child.on("error", fail);
    child.on("exit", (code) => fail(new Error(`The Rust dev worker exited with ${code}`)));
  });
  origin.catch(() => {});
  return { child, origin };
}

function readJson(file: string): Record<string, unknown> | undefined {
  try {
    return JSON.parse(readFileSync(file, "utf8"));
  } catch {
    return undefined;
  }
}

/** The dependencies of the app at `root` that publish source through the `reze` export condition. */
function sourcePackages(root: string): string[] {
  const manifest = readJson(join(root, "package.json"));
  const names = Object.keys({ ...(manifest?.dependencies as object), ...(manifest?.devDependencies as object) });
  return names.filter((name) => {
    const exports = readJson(join(root, "node_modules", name, "package.json"))?.exports;
    if (!exports || typeof exports !== "object") return false;
    return "reze" in exports || Object.values(exports).some((entry) => entry && typeof entry === "object" && "reze" in entry);
  });
}

/** The client build of each Vite root: Vite creates a plugin instance for each environment when it loads a config file. */
const builds = new Map<string, Built>();

export default function reze(options: RezeOptions = {}): Plugin {
  let root = process.cwd();
  let dev = false;
  let base = "/";
  let output: Promise<ProjectResult> | undefined;
  let packageFiles: Map<string, string> | undefined;
  const refs = new Map<string, string>();
  let dispatcherCode = "";
  let streamCode = "";
  let navRef = "";
  let manifestRef = "";
  let devHandler: Promise<Handler> | undefined;

  const routesDir = options.routesDir ?? "src/routes";
  const spa = options.target === "spa";
  const generated = async (name: string, source: string) => {
    const file = join(root, dirname(routesDir), name);
    if ((await readFile(file, "utf8").catch(() => "")) !== source) await writeFile(file, source);
  };
  let rust = false;
  const features = (options.rust && options.rust.features) || [];
  let worker: { child: ChildProcess; origin: Promise<string> } | undefined;
  /** Whether the dev worker runs stale code: a `.rs` file, `Cargo.toml`, or `.reze/main.rs` changed. */
  let stale = true;
  const compile = () =>
    (output ??= compileProjectAsync({ root }, { routesDir, chunks: options.chunks, target: options.target, dev, rust }).then(
      async (result) => {
        await generated("routes.gen.d.ts", result.types);
        if (result.rust) {
          const dir = join(root, ".reze");
          await write(join(dir, ".gitignore"), "*\n");
          for (const [path, source] of Object.entries(result.rust.types)) await write(join(dir, "types", path), source);
          if (result.rust.main && (await write(join(dir, "main.rs"), result.rust.main))) stale = true;
          if (dev) await write(join(dir, "manifest.json"), "null\n");
        }
        return result;
      },
    ));
  /** The origin of the dev worker, which restarts when its code changed. */
  const workerOrigin = async () => {
    await compile();
    if (stale || !worker) {
      stale = false;
      worker?.child.kill();
      worker = startWorker(root, features);
    }
    return worker.origin;
  };
  /** The key of a compiled module in the compiler's output, from its file path. */
  const moduleKey = async (path: string) => {
    const result = await compile();
    packageFiles ??= new Map(
      Object.keys(result.modules)
        .filter((key) => key.startsWith("node_modules/"))
        .map((key) => [realpathSync(join(root, key)), key]),
    );
    const file = packageFiles.get(path) ?? relative(root, path).replaceAll("\\", "/");
    return file in result.modules ? file : undefined;
  };
  /** The keys of the client chunks, in the order of the build manifest. */
  const chunkKeys = async () => Object.keys((await compile()).chunks);
  const buildId = async () => {
    const result = await compile();
    return createHash("sha256")
      .update(result.app)
      .update(result.navigation)
      .update(JSON.stringify(result.chunks))
      .digest("base64url")
      .slice(0, 10);
  };

  return {
    name: "reze",
    enforce: "pre",
    onLog(_, log) {
      if (log.code === "INEFFECTIVE_DYNAMIC_IMPORT" && /runtime\/render\.[jt]s/.test(log.message)) return false;
    },
    config(config, env) {
      const outDirs = {
        client: config.environments?.client?.build?.outDir ?? "dist/client",
        ssr: config.environments?.ssr?.build?.outDir ?? "dist/server",
      };
      const appRoot = resolve(config.root ?? process.cwd());
      const withRust = options.rust !== false && existsSync(join(appRoot, "Cargo.toml"));
      const shared = {
        resolve: { conditions: ["reze", ...defaultClientConditions], dedupe: ["reze-js"] },
        ssr: { noExternal: ["reze-js", ...sourcePackages(appRoot)], resolve: { conditions: ["reze", ...defaultServerConditions] } },
      };
      if (env.command !== "build") {
        return {
          ...shared,
          appType: "custom",
          server: withRust ? { watch: { ignored: ["**/target/**", "**/.reze/**"] } } : undefined,
          environments: { client: { build: { outDir: outDirs.client } }, ssr: { build: { outDir: outDirs.ssr } } },
        };
      }
      const server = existsSync(join(appRoot, "src/server.ts")) ? "/src/server.ts" : "reze-js/server";
      return {
        ...shared,
        builder: {
          async buildApp(builder) {
            await builder.build(builder.environments.client!);
            if (spa) return;
            await builder.build(builder.environments.ssr!);
            const outDir = (name: string) => resolve(appRoot, builder.environments[name]!.config.build.outDir);
            const entry = (name: string) =>
              import(pathToFileURL([".js", ".mjs"].map((ext) => join(outDir("ssr"), name + ext)).find(existsSync)!).href);
            const built = builds.get(root);
            if (rust && built) {
              const result = await compile();
              if (!result.rust?.main) throw new Error("The Rust server has errors: see the diagnostics above");
              await write(join(root, ".reze/manifest.json"), `${JSON.stringify({ build: await buildId(), ...built }, null, 2)}\n`);
              const binary = await cargoBuild(root, features);
              await copyFile(binary, join(outDir("ssr"), basename(binary)));
              return;
            }
            const paths: string[] = await (await entry("prerender")).default();
            if (paths.length === 0) return;
            const server = await entry("server");
            const handler: (request: Request) => Promise<Response> = "createHandler" in server ? server.createHandler() : server.default;
            for (const path of paths) {
              const response = await handler(new Request(new URL(path, "http://localhost"), { headers: { "rz-prerender": "1" } }));
              if (response.status !== 200 || !response.headers.get("content-type")?.startsWith("text/html")) continue;
              const file = join(outDir("client"), path, "index.html");
              await mkdir(dirname(file), { recursive: true });
              await writeFile(file, await response.text());
            }
          },
        },
        environments: {
          client: { build: { outDir: outDirs.client, rollupOptions: { input: [] } } },
          ssr: {
            build: {
              outDir: outDirs.ssr,
              ssr: true,
              rollupOptions: { input: { server, prerender } },
            },
          },
        },
      };
    },
    configResolved(config) {
      root = config.root;
      base = config.base;
      dev = config.command === "serve";
      rust = options.rust !== false && existsSync(join(root, "Cargo.toml"));
    },
    async buildStart() {
      if (dev || this.environment.name !== "client") return;
      refs.clear();
      builds.delete(root);
      for (const node of Object.keys((await compile()).chunks)) {
        refs.set(node, this.emitFile({ type: "chunk", id: `${route}${node}`, name: `route-${node}`, preserveSignature: "exports-only" }));
      }
      navRef = spa
        ? this.emitFile({ type: "chunk", id: spaEntry, name: "app", preserveSignature: "exports-only" })
        : this.emitFile({ type: "chunk", id: nav, name: "navigate", preserveSignature: "exports-only" });
      manifestRef = this.emitFile({ type: "chunk", id: manifest, name: "chunks", preserveSignature: "exports-only" });
      const standalone = async (specifier: string) => {
        const file = (await this.resolve(specifier, join(root, "index.html")))!.id;
        let code = await readFile(file, "utf8");
        if (file.endsWith(".ts")) code = (await transformWithOxc(code, file)).code;
        return (await minify(file, code, { module: true, mangle: { toplevel: true } })).code.replace(/export\s*\{\s*\};?\s*$/, "");
      };
      dispatcherCode = await standalone("reze-js/runtime/dispatcher");
      streamCode = await standalone("reze-js/runtime/stream");
    },
    generateBundle() {
      if (dev || this.environment.name !== "client") return;
      const dispatcherRef = this.emitFile({
        type: "asset",
        name: "dispatcher.js",
        source: dispatcherCode.replace(manifest, base + this.getFileName(manifestRef)),
      });
      if (spa) {
        const head = `<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">`;
        const scripts = [dispatcherRef, navRef]
          .map((ref) => `<script type="module" src="${base}${this.getFileName(ref)}"></script>`)
          .join("");
        const preload = `<link rel="modulepreload" href="${base}${this.getFileName(manifestRef)}">`;
        this.emitFile({
          type: "asset",
          fileName: "index.html",
          source: `<!doctype html><html><head>${head}${scripts}${preload}</head><body></body></html>\n`,
        });
        return;
      }
      const streamRef = this.emitFile({ type: "asset", name: "stream.js", source: streamCode });
      builds.set(root, {
        dispatcher: base + this.getFileName(dispatcherRef),
        modules: base + this.getFileName(manifestRef),
        stream: base + this.getFileName(streamRef),
        chunks: Object.fromEntries([...refs.keys()].map((node, index) => [node, index])),
      });
    },
    configureServer(server) {
      server.httpServer?.on("close", () => worker?.child.kill());
      server.watcher.on("all", (_, file) => {
        const path = relative(root, file);
        if (path.startsWith("..") || path.startsWith(".reze") || file.endsWith("routes.gen.d.ts")) return;
        if (file.endsWith(".rs") || path === "Cargo.toml") stale = true;
        output = undefined;
        packageFiles = undefined;
        devHandler = undefined;
        for (const environment of Object.values(server.environments)) {
          environment.moduleGraph.invalidateAll();
        }
      });
      const entry = existsSync(join(root, "src/server.ts")) ? "/src/server.ts" : "reze-js/server";
      return () => {
        server.middlewares.use(middleware(() => (devHandler ??= server.ssrLoadModule(entry).then(handlerOf))));
      };
    },
    configurePreviewServer(server) {
      const outDir = (name: string) => resolve(root, server.config.environments[name]!.build.outDir);
      const client = outDir("client");
      const entry = [".js", ".mjs"].map((ext) => join(outDir("ssr"), `server${ext}`)).find(existsSync);
      let handler: Promise<Handler> | undefined;
      const shell = (_: unknown, out: ServerResponse, next: (error?: unknown) => void) =>
        readFile(join(client, "index.html")).then((body) => {
          out.writeHead(200, { "content-type": "text/html; charset=utf-8" });
          out.end(body);
        }, next);
      const answer = spa
        ? shell
        : middleware(() => {
            if (!entry) throw new Error("Run `vite build` before `vite preview`");
            return (handler ??= import(pathToFileURL(entry).href).then(handlerOf));
          });
      server.middlewares.use((message, out, next) => {
        const url = new URL(message.url ?? "/", "http://localhost");
        const path = normalize(url.pathname);
        const file = extname(path) ? path : url.search ? "" : join(path, "index.html");
        if (!file || (message.method !== "GET" && message.method !== "HEAD")) return answer(message, out, next);
        readFile(join(client, file))
          .then((body) => {
            out.writeHead(200, { "content-type": types[extname(file)] ?? "text/html; charset=utf-8" });
            out.end(message.method === "HEAD" ? undefined : body);
          })
          .catch(() => answer(message, out, next));
      });
    },
    async resolveId(id, importer) {
      if (spa && importer?.endsWith(serverQuery) && !id.startsWith("\0") && !id.startsWith("virtual:")) {
        const resolved = await this.resolve(id, importer.slice(0, -serverQuery.length), { skipSelf: true });
        return resolved && (await moduleKey(resolved.id)) !== undefined ? `${resolved.id}${serverQuery}` : resolved;
      }
      if (id === spaEntry) return `\0${id}`;
      if ([app, dispatcher, stream, prerender, routes, nav, manifest].includes(id) || id.startsWith(route)) return `\0${id}`;
      if (id.startsWith(rustTypesPrefix)) return `\0${id}`;
      if (importer === resolvedApp && id.startsWith("./"))
        return join(root, id) + (spa && this.environment.name === "client" ? serverQuery : "");
    },
    async load(id) {
      if (id.startsWith(`\0${rustTypesPrefix}`)) return "export {};";
      if (id === `\0${dispatcher}`) return 'import "reze-js/runtime/dispatcher";';
      if (id === `\0${stream}`) return 'import "reze-js/runtime/stream";';
      if (id === `\0${manifest}`) {
        const urls = dev
          ? (await chunkKeys()).map((node) => JSON.stringify(devUrl(route + node)))
          : [...refs.values()].map((ref) => `import.meta.ROLLUP_FILE_URL_${ref}`);
        const navigation = dev ? JSON.stringify(devUrl(nav)) : `import.meta.ROLLUP_FILE_URL_${navRef}`;
        return `import { watch as idle } from "reze-js/runtime/idle";\nimport { links } from "reze-js/runtime/links";\nconst urls = [${urls.join(", ")}];\nlinks(${navigation});\nexport default urls;\nexport const watch = (rz) => idle(rz, urls);\n`;
      }
      if (id === `\0${prerender}`) {
        return `import routes from "${app}";\nimport { prerendered } from "reze-js/runtime/server";\nexport default () => prerendered(routes);\nexport const every = () => prerendered(routes, true);\n`;
      }
      if (id === `\0${spaEntry}`) {
        const client = existsSync(join(root, "src/client.ts"));
        return `import { render } from "reze-js/runtime/render";\nexport { call, fault, navigate, prefetch, submit } from "reze-js/runtime/navigate";\n${client ? 'export { render };\nimport "/src/client.ts";\n' : "render();\n"}`;
      }
      if (id === `\0${nav}`)
        return `export { call, fault, navigate, prefetch, submit } from "${spa ? spaEntry : "reze-js/runtime/navigate"}";`;
      if (id === `\0${routes}`) return `${(await compile()).navigation}export const build = ${JSON.stringify(await buildId())};\n`;
      if (id.startsWith(`\0${route}`)) return (await compile()).chunks[id.slice(route.length + 1)];
      if (id !== resolvedApp) return;
      const result = await compile();
      const errors = result.diagnostics.filter((d) => d.severity === "error");
      if (errors.length > 0) this.error(errors.map((d) => d.rendered).join("\n\n"));
      for (const diagnostic of result.diagnostics) this.warn(diagnostic.rendered);
      const built = dev ? undefined : builds.get(root);
      const assets =
        spa && !dev && this.environment.name === "client"
          ? `const chunks = ${JSON.stringify(Object.fromEntries([...refs.keys()].map((node, index) => [node, index])))};\nexport const assets = { dispatcher: "", modules: "", stream: "", chunk: (node) => chunks[node] };\n`
          : built
            ? `const chunks = ${JSON.stringify(built.chunks)};\nexport const assets = { dispatcher: ${JSON.stringify(built.dispatcher)}, modules: ${JSON.stringify(built.modules)}, stream: ${JSON.stringify(built.stream)}, chunk: (node) => chunks[node] };\n`
            : `export const assets = { dispatcher: ${JSON.stringify(devUrl(dispatcher))}, modules: ${JSON.stringify(devUrl(manifest))}, stream: ${JSON.stringify(devUrl(stream))}, chunk: (node) => ${JSON.stringify(await chunkKeys())}.indexOf(node) };\n`;
      const loaders = rust && dev ? await workerOrigin() : undefined;
      return `${result.app}${assets}export const build = ${JSON.stringify(await buildId())};\nexport const dev = ${dev};\nexport const loaders = ${JSON.stringify(loaders)};\n`;
    },
    async transform(_, id, transformOptions) {
      const file = await moduleKey(id.split("?")[0]!);
      if (file === undefined) return;
      const module = (await compile()).modules[file];
      const code = transformOptions?.ssr || id.endsWith(serverQuery) ? module?.server : module?.client;
      if (code === undefined) return;
      if (file.endsWith(".rs")) return { code: (await transformWithOxc(code, id, { lang: "ts" })).code, map: null };
      return { code, map: null };
    },
  };
}
