import { readdirSync, type Dirent } from "node:fs";
import { join, relative, resolve } from "node:path";

import type { Plugin, ViteDevServer } from "vite";

export interface FileRoutesOptions {
  /** The routes directory, relative to the Vite root; defaults to `src/routes`. */
  dir?: string;
  /** Extensions of route modules; defaults to `.tsx`, `.jsx`, `.ts`, `.js`. */
  extensions?: string[];
}

/** One route of the generated tree: `file` is the module path relative to the routes directory. */
export interface FileRoute {
  path: string;
  file: string;
  children?: FileRoute[];
}

export const RoutesModuleId = "virtual:reze-routes";
const ResolvedRoutesModuleId = `\0${RoutesModuleId}`;
const DefaultExtensions = [".tsx", ".jsx", ".ts", ".js"];

/**
 * Routes from files, as `import routes from "virtual:reze-routes"` for `<Router routes>`. Every
 * module is loaded lazily; its default export is the route component. In a directory:
 *
 * - `index.tsx` is its own path, `about.tsx` is `about`;
 * - `[id].tsx` is `:id`, `[[page]].tsx` is `:page?`, `[...rest].tsx` is `*rest`;
 * - a directory adds its name as a segment; `(group)` adds none;
 * - a module named like a sibling directory (`users.tsx` next to `users/`) is the layout of
 *   that directory's routes, which render in its `<Outlet />`;
 * - names starting with `_` or `.` are skipped.
 *
 * Adding or removing a route module reloads the page in dev.
 */
export function fileRoutes(options: FileRoutesOptions = {}): Plugin {
  const extensions = options.extensions ?? DefaultExtensions;
  let routesDir = "";
  return {
    name: "reze-file-routes",
    configResolved(config) {
      routesDir = resolve(config.root, options.dir ?? "src/routes");
    },
    resolveId(id) {
      return id === RoutesModuleId ? ResolvedRoutesModuleId : undefined;
    },
    load(id) {
      if (id !== ResolvedRoutesModuleId) return undefined;
      this.addWatchFile(routesDir);
      return routesModule(scanRoutes(routesDir, extensions), routesDir);
    },
    configureServer(server) {
      const onChange = (file: string): void => {
        if (isRouteModule(routesDir, file, extensions)) reloadRoutes(server);
      };
      server.watcher.on("add", onChange);
      server.watcher.on("unlink", onChange);
      server.watcher.on("addDir", onChange);
      server.watcher.on("unlinkDir", onChange);
    },
  };
}

function isRouteModule(routesDir: string, file: string, extensions: string[]): boolean {
  const path = relative(routesDir, file);
  if (path.startsWith("..") || path === "") return false;
  return !/\.[^/\\]+$/.test(path) || extensions.some((extension) => path.endsWith(extension));
}

function reloadRoutes(server: ViteDevServer): void {
  for (const environment of Object.values(server.environments)) {
    const module = environment.moduleGraph.getModuleById(ResolvedRoutesModuleId);
    if (module !== undefined) environment.moduleGraph.invalidateModule(module);
  }
  server.ws.send({ type: "full-reload" });
}

/** The route tree of `routesDir`, in a stable order. */
export function scanRoutes(
  routesDir: string,
  extensions: string[] = DefaultExtensions,
): FileRoute[] {
  return scanDirectory(routesDir, "", extensions);
}

function isSkipped(name: string): boolean {
  return name.startsWith("_") || name.startsWith(".");
}

function scanDirectory(root: string, dir: string, extensions: string[]): FileRoute[] {
  const entries = readdirSync(join(root, dir), { withFileTypes: true })
    .filter((entry) => !isSkipped(entry.name))
    .toSorted((a, b) => a.name.localeCompare(b.name));
  const modules = new Map<string, string>();
  for (const entry of entries) {
    const extension = entry.isFile() && extensions.find((ext) => entry.name.endsWith(ext));
    if (extension) modules.set(entry.name.slice(0, -extension.length), join(dir, entry.name));
  }
  const routes: FileRoute[] = [];
  for (const entry of entries.filter((entry: Dirent) => entry.isDirectory())) {
    const children = scanDirectory(root, join(dir, entry.name), extensions);
    const segment = pathSegment(entry.name);
    const layout = modules.get(entry.name);
    if (layout !== undefined) {
      modules.delete(entry.name);
      routes.push({ path: segment, file: toPosix(layout), children });
    } else {
      for (const child of children)
        routes.push({ ...child, path: joinSegments(segment, child.path) });
    }
  }
  for (const [name, file] of modules) {
    routes.push({ path: name === "index" ? "/" : pathSegment(name), file: toPosix(file) });
  }
  return routes;
}

function pathSegment(name: string): string {
  if (/^\(.+\)$/.test(name)) return "";
  const optional = /^\[\[(.+)\]\]$/.exec(name);
  if (optional) return `:${optional[1]}?`;
  const rest = /^\[\.\.\.(.+)\]$/.exec(name);
  if (rest) return `*${rest[1]}`;
  const param = /^\[(.+)\]$/.exec(name);
  return param ? `:${param[1]}` : name;
}

function joinSegments(parent: string, child: string): string {
  if (parent === "") return child;
  return child === "/" ? parent : `${parent}/${child}`;
}

function toPosix(path: string): string {
  return path.replaceAll("\\", "/");
}

/** `export default [...]` of `RouteConfig`s whose `load` imports each module from `routesDir`. */
export function routesModule(routes: FileRoute[], routesDir: string): string {
  const configs = (list: FileRoute[], depth: number): string => {
    const indent = "  ".repeat(depth);
    const items = list.map((route) => {
      const file = JSON.stringify(toPosix(join(routesDir, route.file)));
      const children = route.children ? `, children: ${configs(route.children, depth + 1)}` : "";
      return `${indent}  { path: ${JSON.stringify(route.path)}, load: () => import(${file})${children} }`;
    });
    return items.length === 0 ? "[]" : `[\n${items.join(",\n")},\n${indent}]`;
  };
  return `export default ${configs(routes, 0)};\n`;
}
