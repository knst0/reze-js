import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, join, relative } from "node:path";

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
const route = "virtual:rz-route/";

/** The URL the dev server serves a virtual module at. */
const devUrl = (id: string) => `/@id/__x00__${id}`;

/** URLs of the client build, which the server build reads. */
interface Built {
  dispatcher: string;
  chunks: Record<string, string>;
}

export default function reze(options: RezeOptions = {}): Plugin {
  let root = process.cwd();
  let dev = false;
  let base = "/";
  let output: Promise<ProjectResult> | undefined;
  let built: Built | undefined;
  const refs = new Map<string, string>();
  let dispatcherRef = "";

  const routesDir = options.routesDir ?? "src/routes";
  const compile = () =>
    (output ??= compileProjectAsync({ root }, { routesDir, chunks: options.chunks, dev }).then(async (result) => {
      const file = join(root, dirname(routesDir), "routes.gen.d.ts");
      if ((await readFile(file, "utf8").catch(() => "")) !== result.types) await writeFile(file, result.types);
      return result;
    }));

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
            if (Object.keys((await compile()).chunks).length > 0) await builder.build(builder.environments.client!);
            await builder.build(builder.environments.ssr!);
          },
        },
        environments: {
          client: { build: { outDir: config.environments?.client?.build?.outDir ?? "dist/client", rollupOptions: { input: [] } } },
          ssr: {
            build: { outDir: config.environments?.ssr?.build?.outDir ?? "dist/server", ssr: true, rollupOptions: { input: { server } } },
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
      const file = (await this.resolve("reze-js/runtime/dispatcher", join(root, "index.html")))!.id;
      let code = await readFile(file, "utf8");
      if (file.endsWith(".ts")) code = (await transformWithOxc(code, file)).code;
      dispatcherRef = this.emitFile({
        type: "asset",
        name: "dispatcher.js",
        source: (await minify(file, code, { module: true, mangle: { toplevel: true } })).code,
      });
    },
    generateBundle() {
      if (dev || this.environment.name !== "client") return;
      built = {
        dispatcher: base + this.getFileName(dispatcherRef),
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
      if (id === app || id === dispatcher || id.startsWith(route)) return `\0${id}`;
      if (importer === resolvedApp && id.startsWith("./")) return join(root, id);
    },
    async load(id) {
      if (id === `\0${dispatcher}`) return 'import "reze-js/runtime/dispatcher";';
      if (id.startsWith(`\0${route}`)) return (await compile()).chunks[id.slice(route.length + 1)];
      if (id !== resolvedApp) return;
      const result = await compile();
      const errors = result.diagnostics.filter((d) => d.severity === "error");
      if (errors.length > 0) this.error(errors.map((d) => d.rendered).join("\n\n"));
      for (const diagnostic of result.diagnostics) this.warn(diagnostic.rendered);
      const assets = built
        ? `const chunks = ${JSON.stringify(built.chunks)};\nexport const assets = { dispatcher: ${JSON.stringify(built.dispatcher)}, chunk: (node) => chunks[node] };\n`
        : `export const assets = { dispatcher: ${JSON.stringify(devUrl(dispatcher))}, chunk: (node) => ${JSON.stringify(devUrl(route))} + node };\n`;
      return `${result.app}${assets}export const dev = ${dev};\n`;
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
