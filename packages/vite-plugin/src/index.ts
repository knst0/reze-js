import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

import { compile } from "@rezejs/compiler";
import type { Environment, Plugin } from "vite";

import { createFileRoutesPlugin, type FileRoutesApi, type FileRoutesOptions } from "./routes";

export interface Options {
  diagnostics?: {
    /** File every diagnostic, `info` included, is appended to as one JSON line. */
    jsonl?: string;
  };
  /** Module exporting `link`, e.g. `"@rezejs/router"`: native `<a href>` elements are claimed and passed to it. */
  links?: string;
  /** File extensions the transform compiles, replacing `DEFAULT_ROUTE_EXTENSIONS`. Spread it to extend, e.g. `[...DEFAULT_ROUTE_EXTENSIONS, ".mdx"]` when a preprocessor runs before this plugin. File routes default to this list. */
  extensions?: string[];
  /** File-system routes served after this plugin; `true` is `@rezejs/router/fs` defaults. The result is awaitable in `plugins`. */
  fileRoutes?: boolean | FileRoutesOptions;
}

export interface RezeApi {
  /** Claims native `<a href>` elements for `module`, as the `links` option does. */
  claimLinks(module: string): void;
}

interface Position {
  offset: number;
  line: number;
  column: number;
}

interface Diagnostic {
  code: string;
  severity: "error" | "warn" | "info";
  message: string;
  file: string;
  start: Position;
  end: Position;
  path: string[];
  labels: { start: number; end: number; message: string }[];
  fixes: { title: string; edits: { start: number; end: number; text: string }[] }[];
  data: Record<string, string>;
  docs: string;
  rendered: string;
}

const SkillGuide = "node_modules/@rezejs/compiler/skills/reze-compiler-diagnostics/SKILL.md";
const QueryOrHash = /[?#].*$/;

/** File extensions the transform compiles by default; spread it to extend the list instead of replacing it. */
export const DEFAULT_ROUTE_EXTENSIONS: readonly string[] = [".ts", ".tsx", ".js", ".jsx", ".mts", ".cts", ".mjs", ".cjs"];

function normalizeExtension(e: string): string {
  return (e.startsWith(".") ? e : `.${e}`).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function transformFilter(extras: readonly string[] | undefined): { id: { include: RegExp; exclude: RegExp } } {
  const list = extras === undefined || extras.length === 0 ? DEFAULT_ROUTE_EXTENSIONS : extras;
  const include = new RegExp(`(?:${list.map(normalizeExtension).join("|")})(?:$|\\?)`);
  return { id: { include, exclude: /\/node_modules\// } };
}

function rezePlugin(options: Options): Plugin<RezeApi> {
  const jsonl = options.diagnostics?.jsonl;
  const seenCodes = new Set<string>();
  let jsonlDirReady = false;
  let isServe = false;
  let debugNames = false;
  let hot = false;
  let links = options.links;

  function formatDiagnostic(d: Diagnostic): string {
    if (seenCodes.has(d.code)) return d.rendered;
    seenCodes.add(d.code);
    return `${d.rendered.trimEnd()}
  repair guide: ${SkillGuide}#${d.code.toLowerCase()}
                ${d.docs}`;
  }

  function record(diagnostics: Diagnostic[]): void {
    if (jsonl === undefined || diagnostics.length === 0) return;
    if (!jsonlDirReady) {
      mkdirSync(dirname(jsonl), { recursive: true });
      jsonlDirReady = true;
    }
    appendFileSync(jsonl, diagnostics.map((d) => JSON.stringify(d) + "\n").join(""));
  }

  function emitsSourceMap(config: Environment["config"]): boolean {
    if (!isServe) return Boolean(config.build.sourcemap);
    const sourcemap = config.dev.sourcemap;
    return sourcemap === true || (typeof sourcemap === "object" && sourcemap.js !== false);
  }

  return {
    name: "reze-js",
    enforce: "pre",
    api: {
      claimLinks(module) {
        links = module;
      },
    },
    applyToEnvironment: (environment) => environment.config.consumer === "client",
    configResolved(config) {
      isServe = config.command === "serve";
      debugNames = !config.isProduction;
      hot = isServe && config.server.hmr !== false;
    },
    transform: {
      filter: transformFilter(options.extensions),
      handler(code, id) {
        const result = compile(code, id.replace(QueryOrHash, ""), {
          sourceMap: emitsSourceMap(this.environment.config),
          debugNames,
          hot,
          links,
        });
        if (result === null) return null;
        const diagnostics: Diagnostic[] = result.diagnostics;
        record(diagnostics);
        const errors: Diagnostic[] = [];
        for (const d of diagnostics) {
          if (d.severity === "error") errors.push(d);
          else if (d.severity === "warn") {
            this.warn({
              message: formatDiagnostic(d),
              id,
              loc: { file: d.file, line: d.start.line, column: d.start.column },
            });
          }
        }
        if (errors.length > 0) {
          const first = errors[0]!;
          throw Object.assign(new Error(errors.map(formatDiagnostic).join("\n\n")), {
            id,
            loc: { file: first.file, line: first.start.line, column: first.start.column },
            frame: first.rendered,
            plugin: "reze-js",
            diagnostics: errors,
          });
        }
        return { code: result.code!, map: result.map ?? null };
      },
    },
  };
}

export type { FileRoutesOptions } from "./routes";

export default function reze(options?: Options & { fileRoutes?: false | undefined }): Plugin<RezeApi>;
export default function reze(options: Options & { fileRoutes: true | FileRoutesOptions }): Promise<Plugin[]>;
export default function reze(options: Options = {}): Plugin<RezeApi> | Promise<Plugin[]> {
  const plugin = rezePlugin(options);
  if (options.fileRoutes === undefined || options.fileRoutes === false) return plugin;
  const routesOptions = options.fileRoutes === true ? {} : options.fileRoutes;
  return routesPlugins(plugin, routesOptions, options.extensions);
}

// Optional peer: a static import would make every user install @rezejs/router.
async function loadRouterFs(): Promise<FileRoutesApi> {
  try {
    return await import("@rezejs/router/fs");
  } catch {
    throw new Error("[reze] fileRoutes needs @rezejs/router to be installed");
  }
}

async function routesPlugins(plugin: Plugin<RezeApi>, options: FileRoutesOptions, extras?: readonly string[]): Promise<Plugin[]> {
  const fs = await loadRouterFs();
  if (options.links !== false) plugin.api?.claimLinks("@rezejs/router");
  return [plugin, createFileRoutesPlugin(fs, options, extras)];
}
