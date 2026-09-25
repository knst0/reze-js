import { lazy } from "@rezejs/dom";

import type { RouteConfig } from "./router";

/** The `route` export of a route module: navigation waits for `preload` next to the module. */
export interface FileRouteConfig {
  readonly preload?: () => Promise<unknown>;
}

/**
 * A delivered `$component` ref: code-split modules carry `import`, eagerly delivered ones
 * (`codeSplitting: false`, `server: true`) carry `require`.
 */
export interface FileRouteComponentRef {
  readonly src?: string;
  readonly import?: () => Promise<Record<string, unknown>>;
  readonly require?: () => Record<string, unknown>;
}

/** One `pageRoutes` node from `virtual:file-routes`, path relative to its parent. */
export interface FileRouteEntry {
  readonly path: string;
  readonly page?: boolean;
  readonly $component?: FileRouteComponentRef;
  readonly $$route?: {
    readonly require: () => { readonly route?: FileRouteConfig };
  };
  readonly children?: readonly FileRouteEntry[];
  readonly [key: string]: unknown;
}

/**
 * Turns `pageRoutes` into `Router(routes)`. Each page becomes a lazy route component, so
 * navigation preloads it; a `route.preload` export runs next to the module load.
 *
 * ```ts
 * import { fileRoutes } from "filesystem-routing/vite";
 * import reze from "@rezejs/vite-plugin";
 *
 * export default defineConfig({ plugins: [reze(), fileRoutes()] });
 * ```
 *
 * ```tsx
 * import { pageRoutes } from "virtual:file-routes";
 * import { fileRoutes } from "@rezejs/router/fs";
 * import { Router } from "@rezejs/router";
 *
 * export const App = () => <Router routes={fileRoutes(pageRoutes)} />;
 * ```
 */
export function fileRoutes(entries: readonly FileRouteEntry[]): RouteConfig[] {
  const routes: RouteConfig[] = [];
  for (const entry of entries) {
    const component = fileComponent(entry);
    if (component === undefined) continue;
    routes.push({
      path: entry.path,
      component,
      children: entry.children === undefined ? undefined : fileRoutes(entry.children),
    });
  }
  return routes;
}

function fileComponent(entry: FileRouteEntry): RouteConfig["component"] {
  const ref = entry.$component;
  if (ref === undefined || (ref.import === undefined && ref.require === undefined)) {
    return undefined;
  }
  const component = lazy(
    () => (ref.import === undefined ? Promise.resolve(ref.require?.() ?? {}) : ref.import()),
    { export: "default" },
  );
  const preload = entry.$$route?.require().route?.preload;
  if (typeof preload !== "function") return component;
  const loadModule = component.preload;
  return Object.assign(component, {
    preload: () => Promise.all([loadModule(), preload()]).then(([module]) => module),
  });
}
