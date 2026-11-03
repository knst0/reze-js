import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { type Chunks, compileProjectAsync, type ProjectResult } from "@rezejs/compiler";
import { minify, type Plugin, transformWithOxc } from "vite";

export interface RezeOptions {
  /** Directory of route modules, relative to the Vite root. */
  routesDir?: string;
  chunks?: Chunks;
}

const app = "virtual:rz-app";
const resolvedApp = `\0${app}`;
const dispatcher = "virtual:rz-dispatcher";
const stream = "virtual:rz-stream";
const route = "virtual:rz-route/";
const routes = "virtual:rz-routes";
const nav = "virtual:rz-nav";
const prerender = "virtual:rz-prerender";

/** The URL the dev server serves a virtual module at. */
const devUrl = (id: string) => `/@id/__x00__${id}`;

/** URLs of the client build, which the server build reads. */
interface Built {
  dispatcher: string;
  stream: string;
  chunks: Record<string, string>;
}

export default function reze(options: RezeOptions = {}): Plugin {
  let root = process.cwd();
  let dev = false;
  let base = "/";
  let output: Promise<ProjectResult> | undefined;
  let built: Built | undefined;
  const refs = new Map<string, string>();
  let dispatcherCode = "";
  let streamCode = "";
  let navRef = "";

  const routesDir = options.routesDir ?? "src/routes";
  const compile = () =>
    (output ??= compileProjectAsync({ root }, { routesDir, chunks: options.chunks, dev }).then(async (result) => {
      const file = join(root, dirname(routesDir), "routes.gen.d.ts");
      if ((await readFile(file, "utf8").catch(() => "")) !== result.types) await writeFile(file, result.types);
      return result;
    }));
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
    config(config, env) {
      if (env.command !== "build") return;
      const appRoot = config.root ?? process.cwd();
      const server = existsSync(join(appRoot, "src/server.ts")) ? "/src/server.ts" : "reze-js/server";
      return {
        builder: {
          async buildApp(builder) {
            await builder.build(builder.environments.client!);
            await builder.build(builder.environments.ssr!);
            const outDir = (name: string) => resolve(appRoot, builder.environments[name]!.config.build.outDir);
            const entry = (name: string) =>
              import(pathToFileURL([".js", ".mjs"].map((ext) => join(outDir("ssr"), name + ext)).find(existsSync)!).href);
            const paths: string[] = await (await entry("prerender")).default();
            if (paths.length === 0) return;
            const built = await entry("server");
            const handler: (request: Request) => Promise<Response> = "createHandler" in built ? built.createHandler() : built.default;
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
          client: { build: { outDir: config.environments?.client?.build?.outDir ?? "dist/client", rollupOptions: { input: [] } } },
          ssr: {
            build: {
              outDir: config.environments?.ssr?.build?.outDir ?? "dist/server",
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
    },
    async buildStart() {
      if (dev || this.environment.name !== "client") return;
      refs.clear();
      for (const node of Object.keys((await compile()).chunks)) {
        refs.set(node, this.emitFile({ type: "chunk", id: `${route}${node}`, name: `route-${node}`, preserveSignature: "exports-only" }));
      }
      navRef = this.emitFile({ type: "chunk", id: nav, name: "navigate", preserveSignature: "exports-only" });
      const standalone = async (specifier: string) => {
        const file = (await this.resolve(specifier, join(root, "index.html")))!.id;
        let code = await readFile(file, "utf8");
        if (file.endsWith(".ts")) code = (await transformWithOxc(code, file)).code;
        return (await minify(file, code, { module: true, mangle: { toplevel: true } })).code;
      };
      dispatcherCode = await standalone("reze-js/runtime/dispatcher");
      streamCode = await standalone("reze-js/runtime/stream");
    },
    generateBundle() {
      if (dev || this.environment.name !== "client") return;
      const dispatcherRef = this.emitFile({
        type: "asset",
        name: "dispatcher.js",
        source: dispatcherCode.replace(nav, base + this.getFileName(navRef)),
      });
      const streamRef = this.emitFile({ type: "asset", name: "stream.js", source: streamCode });
      built = {
        dispatcher: base + this.getFileName(dispatcherRef),
        stream: base + this.getFileName(streamRef),
        chunks: Object.fromEntries([...refs].map(([node, ref]) => [node, base + this.getFileName(ref)])),
      };
    },
    configureServer(server) {
      server.watcher.on("all", (_, file) => {
        if (relative(root, file).startsWith("..") || file.endsWith("routes.gen.d.ts")) return;
        output = undefined;
        for (const environment of Object.values(server.environments)) {
          environment.moduleGraph.invalidateAll();
        }
      });
    },
    resolveId(id, importer) {
      if (id === app || id === dispatcher || id === stream || id === prerender || id === routes || id === nav || id.startsWith(route))
        return `\0${id}`;
      if (importer === resolvedApp && id.startsWith("./")) return join(root, id);
    },
    async load(id) {
      if (id === `\0${dispatcher}`) return 'import "reze-js/runtime/dispatcher";';
      if (id === `\0${stream}`) return 'import "reze-js/runtime/stream";';
      if (id === `\0${prerender}`) {
        return `import routes from "${app}";\nimport { prerendered } from "reze-js/runtime/server";\nexport default () => prerendered(routes);\n`;
      }
      if (id === `\0${nav}`) return 'export { navigate, prefetch, submit } from "reze-js/runtime/navigate";';
      if (id === `\0${routes}`) return `${(await compile()).navigation}export const build = ${JSON.stringify(await buildId())};\n`;
      if (id.startsWith(`\0${route}`)) return (await compile()).chunks[id.slice(route.length + 1)];
      if (id !== resolvedApp) return;
      const result = await compile();
      const errors = result.diagnostics.filter((d) => d.severity === "error");
      if (errors.length > 0) this.error(errors.map((d) => d.rendered).join("\n\n"));
      for (const diagnostic of result.diagnostics) this.warn(diagnostic.rendered);
      const assets = built
        ? `const chunks = ${JSON.stringify(built.chunks)};\nexport const assets = { dispatcher: ${JSON.stringify(built.dispatcher)}, stream: ${JSON.stringify(built.stream)}, chunk: (node) => chunks[node] };\n`
        : `export const assets = { dispatcher: ${JSON.stringify(devUrl(dispatcher))}, stream: ${JSON.stringify(devUrl(stream))}, chunk: (node) => ${JSON.stringify(devUrl(route))} + node };\n`;
      return `${result.app}${assets}export const build = ${JSON.stringify(await buildId())};\nexport const dev = ${dev};\n`;
    },
    async transform(_, id, transformOptions) {
      const file = relative(root, id.split("?")[0]!).replaceAll("\\", "/");
      if (file.startsWith("..")) return;
      const module = (await compile()).modules[file];
      const code = transformOptions?.ssr ? module?.server : module?.client;
      if (code === undefined) return;
      return { code, map: null };
    },
  };
}
