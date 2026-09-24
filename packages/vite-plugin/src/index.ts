import { appendFileSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, posix, relative, resolve, sep } from "node:path";

import {
  compile,
  link,
  summarize,
  verify,
  type Diagnostic,
  type LinkModule,
  type LinkResult,
} from "@rezejs/compiler";
import type { EnvironmentModuleNode, Plugin, ResolvedConfig, Rollup, ViteDevServer } from "vite";

export interface Options {
  /**
   * Modules to compile.
   * @default /\.[jt]sx(?:$|\?)/
   * */
  include?: RegExp;
  /**
   * Modules to skip.
   * @default /\/node_modules\//
   * */
  exclude?: RegExp;
  /**
   * Module the compiled code imports its runtime from.
   * @default `"reze-js"`.
   * */
  moduleName?: string;
  /**
   * Emit source maps. `false` skips the map builder in the
   * native compiler (C27), which measurably speeds up large files.
   *
   * @default true
   */
  sourcemap?: boolean;
  /**
   * Enables the compiler optimizations gated behind `optimize` (constant signals, dead JSX branches,
   * inlined computeds, unproxied stores).
   * @default true
   * */
  optimize?: boolean;
  /**
   * Server-side rendering with hydration. SSR transforms always compile for the `server` target
   * (HTML strings for `renderToString`); with `hydratable`, browser transforms compile for
   * `hydrate` (adopting that HTML with `hydrate`) instead of `client`.
   *
   * @default false
   */
  hydratable?: boolean;
  /**
   * Whole-program analysis: every file under `config.root` whose path matches `include`
   * (default `/\.[cm]?[jt]sx?$/`) and not `exclude` is scanned, and cross-module folds, static
   * components and islands are decided for all of them together. Always on in `vite build`.
   * `vite serve` analyzes the program only with `dev: true` (entries: the module scripts of every
   * `*.html` under the root), keeps it up to date from the file watcher, and turns HMR into a
   * full reload when an edit touches a module whose exports the program rewrote, an island or a
   * root, or changes how a component is classified.
   *
   * @default true
   */
  program?: boolean | { include?: RegExp; exclude?: RegExp; dev?: boolean };
  /**
   * Ship only the islands: under `renderToString`/`hydrate` roots, static components render on
   * the server only and the browser hydrates the client components they contain. Requires
   * `hydratable`.
   *
   * @default false
   */
  islands?: boolean;
  /**
   * Keep Vite's `modulepreload` polyfill. Every browser Reze targets supports `modulepreload`,
   * so the plugin turns the polyfill off unless this is `true` or `build.modulePreload` is set.
   *
   * @default false
   */
  modulePreloadPolyfill?: boolean;
  /** Forces runtime features on regardless of what the program uses. */
  features?: Partial<Record<"hydration" | "loading", true>>;
  diagnostics?: {
    /** File every diagnostic (all severities, including `info`) is appended to as one JSON line. */
    jsonl?: string;
  };
}

/** Rollup-style location: what the Vite overlay jumps to. */
export interface ErrorLocation {
  file: string;
  line: number;
  column: number;
}

/** What the transform throws when the compiler reports errors: the Vite overlay reads `loc`/`frame`/`id`. */
export interface CompileError extends Error {
  id: string;
  loc: ErrorLocation;
  frame: string;
  plugin: "rezejs";
  diagnostics: Diagnostic[];
}

const SkillGuide = "node_modules/@rezejs/compiler/skills/compiler-diagnostics/SKILL.md";
const RuntimePackages = ["reze-js", "@rezejs/dom", "@rezejs/signals"];
const Flags = { hydration: "__REZE_HYDRATION__", loading: "__REZE_LOADING__" } as const;
const FlagPattern = /\b__REZE_([A-Z]+)__\b/g;
const ProgramFiles = /\.[cm]?[jt]sx?$/;
const ScriptTag = /<script\b([^>]*)>([\s\S]*?)<\/script\s*>/gi;
const ModuleScript = /(?:^|\s)type\s*=\s*["']?module["'\s]?/i;
const ScriptSource = /(?:^|\s)src\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))/i;
const StaticImport = /\bimport\s*(?:[\w$*{},\s]+?\s*from\s*)?["']([^"']+)["']/g;
const DiagnosticOnlyFacts: Record<string, true> = {
  version: true,
  source_hash: true,
  related: true,
  span: true,
  message: true,
  cause: true,
};

/**
 * The console text of `d`: its rendered block, plus the repair-guide footer the first time
 * `d.code` shows up in `seenCodes` (which it then records).
 */
export function formatDiagnostic(d: Diagnostic, seenCodes: Set<string>): string {
  if (seenCodes.has(d.code)) return d.rendered;
  seenCodes.add(d.code);
  return `${d.rendered.trimEnd()}
  repair guide: ${SkillGuide}#${d.code.toLowerCase()}
                ${d.docs}`;
}

/** The program of one build or dev session: facts per module, the flags it decided, its import edges. */
interface Program {
  linked: LinkResult;
  facts: Map<string, string>;
  imports: Map<string, string[]>;
}

/** One dev program module: its source, summary, and what each specifier resolved to (in or out of the program). */
interface DevModule {
  code: string;
  summary: string;
  specifiers: string[];
  targets: (string | null)[];
}

type WatchEvent = "add" | "change" | "unlink";

/** What one file change does to the dev server. */
interface HotDecision {
  reload: boolean;
  affected: Set<string>;
  invalidated: Set<string>;
}

/** The parts of `ModuleFacts` HMR inspects. */
interface ClassificationFacts {
  components: { client: unknown }[];
  islands: { id: string }[];
  roots: { islands: { id: string; specifier: string }[] }[];
}

/** Compiles JSX to DOM code with the Reze compiler. TypeScript is left to Vite. */
export default function reze(options: Options = {}): Plugin {
  const { moduleName, sourcemap, optimize, hydratable, islands } = options;
  const include = options.include ?? /\.[jt]sx(?:$|\?)/;
  const exclude = options.exclude ?? /\/node_modules\//;
  const scope = typeof options.program === "object" ? options.program : {};
  const jsonl = options.diagnostics?.jsonl;
  const seenCodes = new Set<string>();
  let jsonlDirReady = false;
  let config: ResolvedConfig | undefined;
  let program: Program | undefined;
  let runtimeDirs: string[] = [];
  const outsideSummaries = new Map<string, string | undefined>();
  const runtimeNames = [...new Set([...RuntimePackages, moduleName ?? "reze-js"])];

  let devServer: ViteDevServer | undefined;
  const devModules = new Map<string, DevModule>();
  const htmlEntries = new Map<string, string[]>();
  const outsideImports = new Map<string, string[]>();
  let devQueue: Promise<unknown> = Promise.resolve();
  let reported: Program | undefined;
  let lastDecision: { key: string; decision: HotDecision } | undefined;

  if (islands && !hydratable) {
    throw new Error("[reze] `islands` needs `hydratable`: islands hydrate server-rendered HTML.");
  }

  function record(diagnostics: Diagnostic[]): void {
    if (!jsonl || diagnostics.length === 0) return;
    if (!jsonlDirReady) {
      mkdirSync(dirname(jsonl), { recursive: true });
      jsonlDirReady = true;
    }
    appendFileSync(jsonl, diagnostics.map((d) => JSON.stringify(d) + "\n").join(""));
  }

  function isRuntime(id: string): boolean {
    return runtimeDirs.some((dir) => id.startsWith(dir + sep) || id.startsWith(dir + "/"));
  }

  function isDevProgram(): boolean {
    return config?.command === "serve" && scope.dev === true && options.program !== false;
  }

  function flagValue(name: string, ssr: boolean): boolean {
    const forced = options.features?.[name as keyof typeof Flags];
    if (forced || !program || config?.command !== "build") return true;
    if (name === "hydration") return ssr || Boolean(hydratable);
    return program.linked.features[name] ?? true;
  }

  /** `__REZE_X__` → `true`/`false`, padded with a comment so no column moves. */
  function replaceFlags(code: string, ssr: boolean): string {
    if (!code.includes("__REZE_")) return code;
    return code.replace(FlagPattern, (identifier, flag: string) => {
      const name = flag.toLowerCase();
      if (!(name in Flags)) return identifier;
      const literal = String(flagValue(name, ssr));
      return (
        literal + "/*" + " ".repeat(Math.max(0, identifier.length - literal.length - 4)) + "*/"
      );
    });
  }

  function isProgramFile(file: string): boolean {
    return (
      (scope.include ?? ProgramFiles).test(file) &&
      !(scope.exclude?.test(file) ?? false) &&
      !isRuntime(file)
    );
  }

  function linkProgram(modules: LinkModule[], root: string): Program {
    const linked = link(modules, { optimize: optimize ?? true, islands: islands ?? false, root });
    const imports = new Map(
      modules.map((m) => [m.id, m.resolved.filter((id): id is string => typeof id === "string")]),
    );
    return { linked, facts: new Map(Object.entries(linked.facts)), imports };
  }

  function compileModule(
    context: Pick<Rollup.TransformPluginContext, "warn">,
    code: string,
    id: string,
    ssr: boolean,
    facts: string | undefined,
  ): { code: string; map: string | null } | null {
    const filename = cleanId(id);
    const isCompiled = facts !== undefined || (include.test(id) && !exclude.test(id));
    const target = ssr ? "server" : hydratable ? "hydrate" : "client";
    const out = isCompiled
      ? compile(code, filename, { moduleName, sourceMap: sourcemap, optimize, target, facts })
      : null;
    if (out === null) {
      const flagged = replaceFlags(code, ssr);
      return flagged === code ? null : { code: flagged, map: null };
    }
    record(out.diagnostics);
    const errors: Diagnostic[] = [];
    for (const d of out.diagnostics) {
      if (d.severity === "error") errors.push(d);
      else if (d.severity === "warn") {
        context.warn({
          message: formatDiagnostic(d, seenCodes),
          id: filename,
          loc: { file: d.file, line: d.start.line, column: d.start.column },
        });
      }
    }
    if (errors.length > 0) throw compileError(errors, filename, seenCodes);
    return { code: replaceFlags(out.code!, ssr), map: out.map ?? null };
  }

  function enqueue<T>(task: () => Promise<T>): Promise<T> {
    const run = devQueue.then(task);
    devQueue = run.catch(() => undefined);
    return run;
  }

  async function devResolve(specifier: string, importer: string): Promise<string | null> {
    const resolved = await devServer!.environments.client.pluginContainer.resolveId(
      specifier,
      importer,
    );
    return resolved ? cleanId(resolved.id) : null;
  }

  async function summarizeDev(file: string, code: string): Promise<DevModule | undefined> {
    const out = summarize(code, file, { moduleName });
    if (!out.summary) return undefined;
    const targets: (string | null)[] = [];
    for (const specifier of out.specifiers) targets.push(await devResolve(specifier, file));
    return { code, summary: out.summary, specifiers: out.specifiers, targets };
  }

  async function htmlModuleEntries(file: string, root: string): Promise<string[]> {
    const html = readSource(file);
    if (html === undefined) return [];
    const entries: string[] = [];
    for (const specifier of moduleScriptSpecifiers(html)) {
      const id = await devResolve(
        specifier.startsWith("/") ? resolve(root, "." + specifier) : specifier,
        file,
      );
      if (id) entries.push(id);
    }
    return entries;
  }

  function relinkDev(root: string): void {
    const entries = new Set([...htmlEntries.values()].flat());
    program = linkProgram(
      [...devModules].map(([id, module]) => ({
        id,
        summary: module.summary,
        resolved: module.targets.map((target) =>
          target !== null && devModules.has(target) ? target : null,
        ),
        isEntry: entries.has(id),
      })),
      root,
    );
  }

  async function scanDev(root: string, outDir: string): Promise<void> {
    devModules.clear();
    htmlEntries.clear();
    outsideImports.clear();
    for (const file of scan(root, outDir)) {
      if (file.endsWith(".html")) {
        htmlEntries.set(file, await htmlModuleEntries(file, root));
      } else if (isProgramFile(file)) {
        const module = await summarizeDev(file, readFileSync(file, "utf8"));
        if (module) devModules.set(file, module);
      }
    }
    relinkDev(root);
    reported = program;
  }

  async function reresolve(stale: (target: string | null) => boolean): Promise<void> {
    for (const [id, module] of devModules) {
      for (const [index, target] of module.targets.entries()) {
        if (stale(target)) module.targets[index] = await devResolve(module.specifiers[index], id);
      }
    }
  }

  async function refresh(file: string, event: WatchEvent): Promise<void> {
    if (!config || !isScannedPath(file, config.root, resolve(config.root, config.build.outDir)))
      return;
    const root = config.root;
    if (file.endsWith(".html")) {
      const entries = event === "unlink" ? undefined : await htmlModuleEntries(file, root);
      if (String(entries) === String(htmlEntries.get(file))) return;
      if (entries) htmlEntries.set(file, entries);
      else htmlEntries.delete(file);
      relinkDev(root);
      return;
    }
    if (!isProgramFile(file)) return;
    const code = event === "unlink" ? undefined : readSource(file);
    const known = devModules.get(file);
    if (code === undefined ? !known : known?.code === code) return;
    const module = code === undefined ? undefined : await summarizeDev(file, code);
    if (module) devModules.set(file, module);
    else devModules.delete(file);
    if (event === "add")
      await reresolve((target) => target === null || !target.includes("/node_modules/"));
    else if (event === "unlink") await reresolve((target) => target === file);
    relinkDev(root);
  }

  function decide(file: string): HotDecision {
    const before = reported!;
    const after = program!;
    reported = after;
    const affected = new Set<string>();
    if (before.facts.has(file) || after.facts.has(file)) {
      const importers = new Map<string, string[]>();
      for (const imports of [before.imports, after.imports]) {
        for (const [importer, targets] of imports) {
          for (const target of targets) {
            const list = importers.get(target);
            if (list) list.push(importer);
            else importers.set(target, [importer]);
          }
        }
      }
      const pending = [file];
      for (let next = pending.pop(); next !== undefined; next = pending.pop()) {
        if (affected.has(next)) continue;
        affected.add(next);
        pending.push(...(importers.get(next) ?? []));
      }
    }
    const changed = [...new Set([...before.facts.keys(), ...after.facts.keys()])].filter(
      (id) => before.facts.get(id) !== after.facts.get(id),
    );
    const pinnedBefore = pinnedModules(before);
    const pinnedAfter = pinnedModules(after);
    const newlyClosed = after.linked.closed.filter((id) => !before.linked.closed.includes(id));
    const exposed = [...outsideImports]
      .filter(([, imported]) => imported.some((id) => newlyClosed.includes(id)))
      .map(([id]) => id);
    const reclassified = (id: string): boolean => {
      const old = before.facts.get(id);
      const current = after.facts.get(id);
      return (
        old !== undefined &&
        current !== undefined &&
        classificationOf(old) !== classificationOf(current)
      );
    };
    const redecided = (id: string): boolean => {
      const old = before.facts.get(id);
      const current = after.facts.get(id);
      return (
        old === undefined || current === undefined || decisionsOf(old) !== decisionsOf(current)
      );
    };
    const reload =
      [...affected].some((id) => pinnedBefore.has(id) || pinnedAfter.has(id) || reclassified(id)) ||
      changed.some((id) => !affected.has(id) && redecided(id)) ||
      exposed.length > 0;
    return { reload, affected, invalidated: new Set([...affected, ...changed, ...exposed]) };
  }

  async function checkClosedImports(
    context: Rollup.TransformPluginContext,
    code: string,
    filename: string,
  ): Promise<void> {
    const out = summarize(code, filename, { moduleName });
    if (!out.summary) return;
    const imported: string[] = [];
    for (const specifier of out.specifiers) {
      const resolved = await context.resolve(specifier, filename);
      if (resolved) imported.push(cleanId(resolved.id));
    }
    outsideImports.set(filename, imported);
    const closed = new Set(program!.linked.closed);
    if (!imported.some((id) => closed.has(id))) return;
    const errors = verify(program!.linked, [{ id: filename, imported }]);
    record(errors);
    if (errors.length > 0) throw compileError(errors, filename, seenCodes);
  }

  async function transformDev(
    context: Rollup.TransformPluginContext,
    code: string,
    id: string,
    ssr: boolean,
  ): Promise<{ code: string; map: string | null } | null> {
    await devQueue;
    const filename = cleanId(id);
    const known = devModules.get(filename);
    if (known ? known.code !== code : isProgramFile(filename)) {
      await enqueue(() => refresh(filename, "change"));
    }
    const facts = program?.facts.get(filename);
    if (facts !== undefined) outsideImports.delete(filename);
    else if (
      program &&
      !id.startsWith("\0") &&
      !isRuntime(filename) &&
      !filename.includes("/node_modules/")
    ) {
      await checkClosedImports(context, code, filename);
    }
    return compileModule(context, code, id, ssr, facts);
  }

  return {
    name: "reze-js",
    enforce: "pre",
    config(userConfig) {
      if (options.modulePreloadPolyfill || userConfig.build?.modulePreload !== undefined) return;
      return { build: { modulePreload: { polyfill: false } } };
    },
    configResolved(resolved) {
      config = resolved;
    },
    configureServer(server) {
      if (!isDevProgram()) return;
      devServer = server;
      const events: [WatchEvent, WatchEvent, WatchEvent] = ["add", "change", "unlink"];
      for (const event of events) {
        server.watcher.on(event, (file: string) => {
          enqueue(() => refresh(file, event)).catch((error: unknown) =>
            server.config.logger.error(String(error)),
          );
        });
      }
    },
    async buildStart() {
      program = undefined;
      outsideSummaries.clear();
      runtimeDirs = [];
      for (const name of runtimeNames) {
        const entry = await this.resolve(name);
        if (entry) runtimeDirs.push(dirname(cleanId(entry.id)));
      }
      if (!config || options.program === false) return;
      const root = config.root;
      const outDir = resolve(root, config.build.outDir);
      if (isDevProgram()) {
        await enqueue(() => scanDev(root, outDir));
        return;
      }
      if (config.command !== "build") return;
      const files = scan(root, outDir).filter((file) => isProgramFile(file));
      const summaries = new Map<string, { summary: string; specifiers: string[] }>();
      for (const file of files) {
        const out = summarize(readFileSync(file, "utf8"), file, { moduleName });
        if (out.summary) summaries.set(file, { summary: out.summary, specifiers: out.specifiers });
      }
      const entries = new Set<string>();
      for (const input of inputsOf(config)) {
        const resolved = await this.resolve(input, undefined);
        if (resolved) entries.add(cleanId(resolved.id));
      }
      const modules: LinkModule[] = [];
      for (const [id, { summary, specifiers }] of summaries) {
        const resolved: (string | null)[] = [];
        for (const specifier of specifiers) {
          const target = await this.resolve(specifier, id);
          const targetId = target && cleanId(target.id);
          resolved.push(targetId && summaries.has(targetId) ? targetId : null);
        }
        modules.push({ id, summary, resolved, isEntry: entries.has(id) });
      }
      program = linkProgram(modules, root);
    },
    transform: {
      filter: { id: { include: /\.[cm]?[jt]sx?(?:$|\?)/ } },
      handler(code, id, transformOptions) {
        const ssr = Boolean(transformOptions?.ssr);
        if (isDevProgram()) return transformDev(this, code, id, ssr);
        const filename = cleanId(id);
        const facts = program?.facts.get(filename);
        if (
          program &&
          !facts &&
          !isRuntime(filename) &&
          runtimeNames.some((name) => code.includes(name))
        ) {
          outsideSummaries.set(filename, summarize(code, filename, { moduleName }).summary);
        }
        return compileModule(this, code, id, ssr, facts);
      },
    },
    async hotUpdate({ type, file, timestamp, modules }) {
      if (!isDevProgram()) return;
      const key = `${file}\0${timestamp}`;
      if (lastDecision?.key !== key) {
        const event: WatchEvent =
          type === "create" ? "add" : type === "delete" ? "unlink" : "change";
        const decision = await enqueue(async () => {
          await refresh(file, event);
          return program && reported ? decide(file) : undefined;
        });
        if (!decision) return;
        lastDecision = { key, decision };
      }
      const { reload, affected, invalidated } = lastDecision.decision;
      const graph = this.environment.moduleGraph;
      if (reload) {
        const seen = new Set<EnvironmentModuleNode>();
        for (const id of invalidated) {
          for (const mod of graph.getModulesByFile(id) ?? [])
            graph.invalidateModule(mod, seen, timestamp, true);
        }
        this.environment.hot.send({ type: "full-reload" });
        return [];
      }
      if (affected.size === 0) return;
      const updated = new Set(modules);
      for (const id of affected)
        for (const mod of graph.getModulesByFile(id) ?? []) updated.add(mod);
      return [...updated];
    },
    buildEnd() {
      if (!program || config?.command !== "build") return;
      const outside = [];
      for (const id of this.getModuleIds()) {
        const filename = cleanId(id);
        if (program.facts.has(filename) || isRuntime(filename) || id.startsWith("\0")) continue;
        const info = this.getModuleInfo(id);
        const imported = [
          ...(info?.importedIds ?? []),
          ...(info?.dynamicallyImportedIds ?? []),
        ].map(cleanId);
        outside.push({ id: filename, summary: outsideSummaries.get(filename), imported });
      }
      const errors = verify(program.linked, outside);
      record(errors);
      if (errors.length > 0) {
        this.error(errors.map((d) => formatDiagnostic(d, seenCodes)).join("\n\n"));
      }
    },
  };
}

function cleanId(id: string): string {
  return id.replace(/[?#].*$/, "");
}

function readSource(file: string): string | undefined {
  try {
    return readFileSync(file, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
    throw error;
  }
}

/** Every file under `root`, skipping `node_modules`, dot directories and the output directory. */
function scan(root: string, outDir: string): string[] {
  const files: string[] = [];
  const walk = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name === "node_modules" || entry.name.startsWith(".") || path === outDir)
          continue;
        walk(path);
      } else if (entry.isFile()) {
        files.push(path);
      }
    }
  };
  walk(root);
  return files;
}

/** Whether `scan(root, outDir)` would visit `file`. */
function isScannedPath(file: string, root: string, outDir: string): boolean {
  const path = relative(root, file);
  if (path.startsWith("..") || !relative(outDir, file).startsWith("..")) return false;
  const directories = path.split(/[\\/]/).slice(0, -1);
  return !directories.some((name) => name === "node_modules" || name.startsWith("."));
}

/** Specifiers of an HTML page's module scripts: `src` of external ones, static imports of inline ones. */
function moduleScriptSpecifiers(html: string): string[] {
  const specifiers: string[] = [];
  for (const [, attributes, body] of html.matchAll(ScriptTag)) {
    if (!ModuleScript.test(attributes)) continue;
    const source = ScriptSource.exec(attributes);
    if (source) specifiers.push(source[1] ?? source[2] ?? source[3]);
    else for (const [, specifier] of body.matchAll(StaticImport)) specifiers.push(specifier);
  }
  return specifiers;
}

const pinnedByProgram = new WeakMap<Program, Set<string>>();

/** Modules whose HMR needs a full reload: the program rewrote their exports, or they hold an island or a root. */
function pinnedModules(program: Program): Set<string> {
  const cached = pinnedByProgram.get(program);
  if (cached) return cached;
  const pinned = new Set(program.linked.closed);
  for (const [id, json] of program.facts) {
    const facts = JSON.parse(json) as ClassificationFacts;
    if (facts.islands.length > 0) pinned.add(id);
    for (const root of facts.roots) {
      pinned.add(id);
      for (const island of root.islands)
        pinned.add(posix.join(posix.dirname(id), island.specifier));
    }
  }
  pinnedByProgram.set(program, pinned);
  return pinned;
}

/** Static/client per component, islands and roots, independent of source offsets. */
function classificationOf(json: string): string {
  const facts = JSON.parse(json) as ClassificationFacts;
  return JSON.stringify([
    facts.components.map((component) => component.client !== null),
    facts.islands.map((island) => island.id),
    facts.roots.map((root) => root.islands.map((island) => island.id)),
  ]);
}

/** The facts that shape the compiled code, without what only feeds diagnostics. */
function decisionsOf(json: string): string {
  return JSON.stringify(JSON.parse(json), (key, value: unknown) =>
    Object.hasOwn(DiagnosticOnlyFacts, key) ? undefined : value,
  );
}

/** Module inputs of the build: `build.ssr` for SSR builds, otherwise `rolldownOptions.input`. */
function inputsOf(config: ResolvedConfig): string[] {
  const ssr = config.build.ssr;
  if (typeof ssr === "string") return [resolve(config.root, ssr)];
  const input = config.build.rolldownOptions.input;
  const inputs =
    typeof input === "string" ? [input] : Array.isArray(input) ? input : Object.values(input ?? {});
  return inputs.filter((i) => !i.endsWith(".html")).map((i) => resolve(config.root, i));
}

function compileError(errors: Diagnostic[], id: string, seenCodes: Set<string>): CompileError {
  const [first] = errors;
  const message = errors.map((d) => formatDiagnostic(d, seenCodes)).join("\n\n");
  return Object.assign(new Error(message), {
    id,
    loc: { file: first.file, line: first.start.line, column: first.start.column },
    frame: first.rendered,
    plugin: "rezejs" as const,
    diagnostics: errors,
  });
}
