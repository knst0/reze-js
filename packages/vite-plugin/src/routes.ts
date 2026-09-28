import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve, sep } from "node:path";

import type * as routerFs from "@rezejs/router/fs";
import type { FileRoute } from "@rezejs/router/fs";
import type { Plugin } from "vite";

export interface FileRoutesOptions {
  /** Routes directory, relative to Vite `root`. Default `"src/routes"`. */
  dir?: string;
  /** Generated declaration file, relative to Vite `root`; `false` disables it. Default `"src/routes.gen.d.ts"`. */
  dts?: string | false;
  /** Claim native `<a href>` elements for the router (`aria-current`, `data-active`, `data-pending`). Default `true`. */
  links?: boolean;
  /** The history the app routes with, which decides the `<a href>` shape the generated types accept: `"/about"` under Vite's `base` for `"browser"`, `"#/about"` for `"hash"`. Default `"browser"`. */
  history?: "browser" | "hash";
}

export type FileRoutesApi = typeof routerFs;

const VirtualId = "virtual:reze-routes";
const ResolvedId = "\0" + VirtualId;

function writeIfChanged(file: string, content: string): void {
  if (existsSync(file) && readFileSync(file, "utf8") === content) return;
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, content);
}

/** File-system routes from `dir`, served as `virtual:reze-routes` with one lazy chunk per route file. */
export function createFileRoutesPlugin(fs: FileRoutesApi, options: FileRoutesOptions): Plugin {
  let dir = "";
  let dtsFile: string | undefined;
  let hrefBase = "";
  let code: string | undefined;

  function scan(): FileRoute[] {
    if (!existsSync(dir)) throw new Error(`[reze-router] routes directory not found: ${dir}`);
    const files: string[] = [];
    for (const entry of readdirSync(dir, { recursive: true, encoding: "utf8" })) {
      if (statSync(join(dir, entry)).isFile()) files.push(entry.split(sep).join("/"));
    }
    return fs.scanRoutes(files);
  }

  function generate(): boolean {
    const routes = scan();
    const next = fs.routesModule(routes, dir, hrefBase);
    if (dtsFile !== undefined) writeIfChanged(dtsFile, fs.routesDts(routes, hrefBase, dtsFile, dir));
    const isChanged = next !== code;
    code = next;
    return isChanged;
  }

  return {
    name: "reze-router",
    configResolved(config) {
      dir = resolve(config.root, options.dir ?? "src/routes");
      const dts = options.dts ?? "src/routes.gen.d.ts";
      dtsFile = dts === false ? undefined : resolve(config.root, dts);
      hrefBase = options.history === "hash" ? "#" : fs.routerBase(config.base);
    },
    buildStart() {
      generate();
    },
    resolveId(id) {
      return id === VirtualId ? ResolvedId : undefined;
    },
    load(id) {
      if (id !== ResolvedId) return undefined;
      if (code === undefined) generate();
      return code;
    },
    configureServer(server) {
      server.watcher.add(dir);
      const onChange = (file: string): void => {
        if (file !== dir && !file.startsWith(dir + sep)) return;
        let isChanged: boolean;
        try {
          isChanged = generate();
        } catch (error) {
          server.config.logger.error(String(error instanceof Error ? error.message : error));
          return;
        }
        if (!isChanged) return;
        const client = server.environments.client;
        const module = client.moduleGraph.getModuleById(ResolvedId);
        if (module !== undefined) client.moduleGraph.invalidateModule(module);
        client.hot.send({ type: "full-reload" });
      };
      server.watcher.on("add", onChange);
      server.watcher.on("unlink", onChange);
      server.watcher.on("addDir", onChange);
      server.watcher.on("unlinkDir", onChange);
    },
  };
}
