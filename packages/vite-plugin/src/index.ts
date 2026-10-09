import { createHash } from "node:crypto";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

import { compile } from "@rezejs/compiler";
import type { DevEnvironment, Environment, EnvironmentModuleNode, Plugin } from "vite";

import { ModuleFactsStore, staticImportSpecifiers, type Resolver } from "./facts";
import { profileHash } from "./hash";
import { canonicalModuleId } from "./module-identity";
import { createFileRoutesPlugin, type FileRoutesApi, type FileRoutesOptions } from "./routes";
import { GeneratedIds, HtmlEnv, SsgViewId } from "./server/adapter";
import type { SsgOptions, SsrOptions } from "./server/options";
import { createServerPlugin, createServerShared } from "./server/plugin";
import type { ServerShared } from "./server/plugin";
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
  /** Static-site generation: a production build that prerenders each page with the server renderer. Absent by default. */
  ssg?: SsgOptions;
  /** Request-time rendering: builds `dist-server/entry.js`, exporting a web `fetch` handler. Absent by default. */
  ssr?: SsrOptions;
  profile?: {
    /** Directory of per-file profiling facts. The dev server files session trees posted to `/__reze/profile` there; later transforms read them back to specialize codegen. */
    dir: string;
  };
  /** Packages whose sources are analyzed for module facts when their `reze.json` manifest is missing or stale, e.g. `"@acme/ui"` or `"@acme/charts/*"`. Other packages stay opaque. */
  analyze?: string[];
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
const RuntimeEntry = /^(?:reze-js|@rezejs\/(?:dom|signals|router))(?:\/|$)/;

/** File extensions the transform compiles by default; spread it to extend the list instead of replacing it. */
export const DEFAULT_ROUTE_EXTENSIONS: readonly string[] = [".ts", ".tsx", ".js", ".jsx", ".mts", ".cts", ".mjs", ".cjs"];

function normalizeExtension(e: string): string {
  return (e.startsWith(".") ? e : `.${e}`).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function transformFilter(extras: readonly string[] | undefined): { include: RegExp; exclude: RegExp } {
  const list = extras === undefined || extras.length === 0 ? DEFAULT_ROUTE_EXTENSIONS : extras;
  const include = new RegExp(`(?:${list.map(normalizeExtension).join("|")})(?:$|\\?)`);
  return { include, exclude: /\/node_modules\// };
}

interface ProfileComponentFacts {
  component: string;
  file: string;
  mounts: number;
  props: number;
  reruns: number;
  writes: number;
}

interface ProfileFile {
  v: 1;
  file: string;
  hash: string;
  components: ProfileComponentFacts[];
}

function profileKey(file: string): string {
  return `${createHash("sha256").update(file).digest("hex").slice(0, 32)}.json`;
}

function readProfileFacts(dir: string, file: string, source: string): ProfileFile | undefined {
  let parsed: unknown;
  try {
    parsed = JSON.parse(readFileSync(join(dir, profileKey(file)), "utf8"));
  } catch {
    return undefined;
  }
  if (typeof parsed !== "object" || parsed === null) return undefined;
  const record = parsed as Partial<ProfileFile>;
  if (record.v !== 1 || record.file !== file || record.hash !== profileHash(source)) return undefined;
  if (!Array.isArray(record.components)) return undefined;
  return { v: 1, file, hash: record.hash, components: record.components };
}

function normalizeCounts(value: unknown): ProfileComponentFacts | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const row = value as Record<string, unknown>;
  if (typeof row.component !== "string" || typeof row.file !== "string") return undefined;
  const counts = [row.mounts, row.props, row.reruns, row.writes];
  if (!counts.every((count) => typeof count === "number" && Number.isFinite(count))) return undefined;
  const [mounts, props, reruns, writes] = counts as number[];
  return {
    component: row.component,
    file: row.file,
    mounts: Math.max(0, Math.floor(mounts!)),
    props: Math.max(0, Math.floor(props!)),
    reruns: Math.max(0, Math.floor(reruns!)),
    writes: Math.max(0, Math.floor(writes!)),
  };
}

function mergeProfileComponents(current: ProfileComponentFacts[], incoming: ProfileComponentFacts[]): ProfileComponentFacts[] {
  const rows = new Map<string, ProfileComponentFacts>();
  for (const row of current) rows.set(`${row.file}#${row.component}`, { ...row });
  for (const row of incoming) {
    const key = `${row.file}#${row.component}`;
    const kept = rows.get(key);
    if (kept === undefined) {
      rows.set(key, { ...row });
    } else {
      kept.mounts += row.mounts;
      kept.props = Math.max(kept.props, row.props);
      kept.reruns += row.reruns;
      kept.writes += row.writes;
    }
  }
  return [...rows.values()].sort((a, b) => (a.file < b.file ? -1 : a.file > b.file ? 1 : a.component < b.component ? -1 : 1));
}

function fileProfileTree(dir: string, hashes: Map<string, string>, tree: unknown): void {
  if (typeof tree !== "object" || tree === null) throw new Error("a profile tree is an object");
  const { components } = tree as { components: unknown };
  if (!Array.isArray(components)) throw new Error("a profile tree needs components");
  const byFile = new Map<string, ProfileComponentFacts[]>();
  for (const value of components) {
    const row = normalizeCounts(value);
    if (row === undefined) throw new Error("a profile row needs component, file and counters");
    const rows = byFile.get(row.file);
    if (rows === undefined) byFile.set(row.file, [row]);
    else rows.push(row);
  }
  if (byFile.size === 0) return;
  let madeDir = false;
  for (const [file, incoming] of byFile) {
    const hash = hashes.get(file);
    if (hash === undefined) continue;
    if (!madeDir) {
      mkdirSync(dir, { recursive: true });
      madeDir = true;
    }
    const path = join(dir, profileKey(file));
    let current: ProfileComponentFacts[] = [];
    try {
      const parsed = JSON.parse(readFileSync(path, "utf8")) as Partial<ProfileFile>;
      if (parsed.v === 1 && parsed.hash === hash && Array.isArray(parsed.components)) current = parsed.components;
    } catch {
      current = [];
    }
    const record: ProfileFile = { v: 1, file, hash, components: mergeProfileComponents(current, incoming) };
    writeFileSync(path, `${JSON.stringify(record)}\n`);
  }
}

type ResolveById = (specifier: string, importer: string) => Promise<{ id: string; external?: unknown } | null>;

function resolverOf(resolveById: ResolveById): Resolver {
  return async (specifier, importer) => {
    const resolved = await resolveById(specifier, importer);
    if (resolved === null || resolved.external || resolved.id.startsWith("\0")) return undefined;
    return resolved.id.replace(QueryOrHash, "");
  };
}

function rezePlugin(options: Options, shared: ServerShared): Plugin<RezeApi> {
  const jsonl = options.diagnostics?.jsonl;
  const seenCodes = new Set<string>();
  let jsonlDirReady = false;
  let isServe = false;
  let debugNames = false;
  let hot = false;
  let links = options.links;
  let root = "";
  const profileDir = options.profile?.dir;
  const profileHashes = new Map<string, string>();
  const runtimeEntries = new Set<string>();
  const runtimeDirectories = new Set<string>();
  const filter = transformFilter(options.extensions);
  const moduleFacts = new ModuleFactsStore(options.analyze ?? [], (file) => filter.include.test(file));

  async function invalidateStaleImporters(environment: DevEnvironment, file: string, timestamp: number): Promise<EnvironmentModuleNode[]> {
    const resolve = resolverOf((specifier, importer) => environment.pluginContainer.resolveId(specifier, importer));
    const graph = environment.moduleGraph;
    const importers = await moduleFacts.importersToInvalidate(file, resolve);
    const nodes = importers.flatMap((importer) => [...(graph.getModulesByFile(importer) ?? [])]);
    for (const node of nodes) graph.invalidateModule(node, new Set(), timestamp, true);
    return nodes;
  }

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
    applyToEnvironment: (environment) => environment.name === "client" || environment.name === HtmlEnv,
    config() {
      return { resolve: { conditions: ["reze"] } };
    },
    configResolved(config) {
      isServe = config.command === "serve";
      debugNames = !config.isProduction;
      hot = isServe && config.server.hmr !== false;
      root = config.root ?? "";
      shared.root = root;
      shared.isServe = isServe;
      moduleFacts.configure({ root, warn: isServe ? (message) => config.logger.warn(message) : undefined });
    },
    async resolveId(id, importer, options) {
      if (!RuntimeEntry.test(id)) return;
      const resolved = await this.resolve(id, importer, { ...options, skipSelf: true });
      if (resolved) {
        const file = resolved.id.replace(QueryOrHash, "");
        runtimeEntries.add(file);
        const dist = file.lastIndexOf("/dist/");
        if (dist !== -1) runtimeDirectories.add(file.slice(0, dist + 6));
      }
      return resolved;
    },
    async transform(code, id) {
      const file = id.replace(QueryOrHash, "");
      if (id !== SsgViewId && (!filter.include.test(id) || (filter.exclude.test(id) && !moduleFacts.declaresReze(file)))) return null;
      if (runtimeEntries.has(file) || GeneratedIds.includes(id)) return;
      if (id.startsWith("\0vite/") || id.startsWith("\0rolldown/")) return;
      for (const directory of runtimeDirectories) if (file.startsWith(directory)) return;
      const profile = profileDir === undefined ? undefined : readProfileFacts(resolve(root, profileDir), file, code);
      if (profileDir !== undefined) profileHashes.set(file, profileHash(code));
      const envName = this.environment.name;
      const target = shared.enabled && shared.root !== "" ? (envName === HtmlEnv ? "html" : "island") : undefined;
      const moduleId = target === undefined ? undefined : canonicalModuleId(id, shared.root);
      if (moduleId !== undefined) {
        shared.registry.register(moduleId, code);
        if (target === "html" && id !== SsgViewId) shared.moduleFiles.set(moduleId, file);
      }
      const imported = await moduleFacts.factsFor(
        file,
        staticImportSpecifiers(code),
        resolverOf((specifier, importer) => this.resolve(specifier, importer, { skipSelf: true })),
      );
      const result = compile(code, file, {
        sourceMap: emitsSourceMap(this.environment.config),
        debugNames,
        hot,
        links,
        ...(profile === undefined ? {} : { profile }),
        ...(target === undefined ? {} : { target, moduleId }),
        facts: imported,
      });
      if (result === null) return null;
      moduleFacts.recordCompiled(file, code, result.facts);
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
    async watchChange(file) {
      if (this.environment.mode === "dev") await invalidateStaleImporters(this.environment, file, Date.now());
    },
    async hotUpdate(context) {
      const importers = await invalidateStaleImporters(this.environment, context.file, context.timestamp);
      if (importers.length === 0) return;
      return [...new Set([...context.modules, ...importers])];
    },
    configureServer(server) {
      if (profileDir === undefined) return;
      const dir = resolve(root, profileDir);
      server.middlewares.use("/__reze/profile", (req, res, next) => {
        if (req.method !== "POST") {
          next();
          return;
        }
        let body = "";
        req.on("data", (chunk: unknown) => {
          body += String(chunk);
        });
        req.on("end", () => {
          try {
            fileProfileTree(dir, profileHashes, JSON.parse(body));
            res.statusCode = 200;
          } catch {
            res.statusCode = 400;
          }
          res.end();
        });
      });
    },
  };
}

export type { FileRoutesOptions } from "./routes";
export type { SsgOptions, SsrOptions, StaticParams, StaticPathsValue } from "./server/options";

export default function reze(options?: Options & { fileRoutes?: false | undefined; ssg?: undefined; ssr?: undefined }): Plugin<RezeApi>;
export default function reze(options: Options & { ssg?: SsgOptions; ssr?: SsrOptions; fileRoutes?: false | undefined }): Plugin[];
export default function reze(
  options: Options & { ssg?: SsgOptions; ssr?: SsrOptions; fileRoutes: true | FileRoutesOptions },
): Promise<Plugin[]>;
export default function reze(options: Options = {}): Plugin<RezeApi> | Plugin[] | Promise<Plugin[]> {
  const shared = createServerShared();
  const plugin = rezePlugin(options, shared);
  const server =
    options.ssg === undefined && options.ssr === undefined
      ? []
      : createServerPlugin({ ssg: options.ssg, ssr: options.ssr }, shared, options.fileRoutes === true ? {} : options.fileRoutes);
  if (options.fileRoutes === undefined || options.fileRoutes === false) {
    return server.length === 0 ? plugin : [plugin, ...server];
  }
  const routesOptions = options.fileRoutes === true ? {} : options.fileRoutes;
  return routesPlugins(plugin, routesOptions, options.extensions).then((plugins) => [...server, ...plugins]);
}

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
