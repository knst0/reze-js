import { appendFileSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve, sep } from "node:path";

import { compile, link, summarize, verify, type Diagnostic, type LinkResult } from "@rezejs/compiler";
import type { Plugin, ResolvedConfig } from "vite";

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
   * Whole-program analysis in `vite build`: every file under `config.root` whose path matches
   * `include` (default `/\.[cm]?[jt]sx?$/`) and not `exclude` is scanned, and cross-module folds,
   * static components and islands are decided for all of them together. Ignored in `vite serve`.
   *
   * @default true
   */
  program?: boolean | { include?: RegExp; exclude?: RegExp };
  /**
   * Ship only the islands: under `renderToString`/`hydrate` roots, static components render on
   * the server only and the browser hydrates the client components they contain. Requires
   * `hydratable`.
   *
   * @default false
   */
  islands?: boolean;
  /** Forces runtime features on regardless of what the program uses. */
  features?: Partial<Record<"hydration" | "suspense", true>>;
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
const Flags = { hydration: "__REZE_HYDRATION__", suspense: "__REZE_SUSPENSE__" } as const;
const FlagPattern = /\b__REZE_([A-Z]+)__\b/g;
const ProgramFiles = /\.[cm]?[jt]sx?$/;

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

/** The program of one build: facts per module and the flags it decided. */
interface Program {
  linked: LinkResult;
  facts: Map<string, string>;
}

/** Compiles JSX to DOM code with the Reze compiler. TypeScript is left to Vite. */
export default function reze(options: Options = {}): Plugin {
  const { moduleName, sourcemap, optimize, hydratable, islands } = options;
  const include = options.include ?? /\.[jt]sx(?:$|\?)/;
  const exclude = options.exclude ?? /\/node_modules\//;
  const jsonl = options.diagnostics?.jsonl;
  const seenCodes = new Set<string>();
  let jsonlDirReady = false;
  let config: ResolvedConfig | undefined;
  let program: Program | undefined;
  let runtimeDirs: string[] = [];
  const outsideSummaries = new Map<string, string | undefined>();
  const runtimeNames = [...new Set([...RuntimePackages, moduleName ?? "reze-js"])];

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

  function flagValue(name: string, ssr: boolean): boolean {
    const forced = options.features?.[name as keyof typeof Flags];
    if (forced || !program) return true;
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
      return literal + "/*" + " ".repeat(Math.max(0, identifier.length - literal.length - 4)) + "*/";
    });
  }

  return {
    name: "reze-js",
    enforce: "pre",
    configResolved(resolved) {
      config = resolved;
    },
    async buildStart() {
      program = undefined;
      outsideSummaries.clear();
      runtimeDirs = [];
      for (const name of runtimeNames) {
        const entry = await this.resolve(name);
        if (entry) runtimeDirs.push(dirname(cleanId(entry.id)));
      }
      if (!config || config.command !== "build" || options.program === false) return;
      const scope = typeof options.program === "object" ? options.program : {};
      const files = scan(config.root, resolve(config.root, config.build.outDir)).filter(
        (file) =>
          (scope.include ?? ProgramFiles).test(file) &&
          !(scope.exclude?.test(file) ?? false) &&
          !isRuntime(file),
      );
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
      const modules = [];
      for (const [id, { summary, specifiers }] of summaries) {
        const resolved: (string | null)[] = [];
        for (const specifier of specifiers) {
          const target = await this.resolve(specifier, id);
          const targetId = target && cleanId(target.id);
          resolved.push(targetId && summaries.has(targetId) ? targetId : null);
        }
        modules.push({ id, summary, resolved, isEntry: entries.has(id) });
      }
      const linked = link(modules, { optimize: optimize ?? true, islands: islands ?? false, root: config.root });
      program = { linked, facts: new Map(Object.entries(linked.facts)) };
    },
    transform: {
      filter: { id: { include: /\.[cm]?[jt]sx?(?:$|\?)/ } },
      handler(code, id, transformOptions) {
        const filename = cleanId(id);
        const ssr = Boolean(transformOptions?.ssr);
        const facts = program?.facts.get(filename);
        const isCompiled = facts !== undefined || (include.test(id) && !exclude.test(id));
        const mentionsRuntime = runtimeNames.some((name) => code.includes(name));
        if (program && !facts && !isRuntime(filename) && mentionsRuntime) {
          const out = summarize(code, filename, { moduleName });
          outsideSummaries.set(filename, out.summary);
        }
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
            this.warn({
              message: formatDiagnostic(d, seenCodes),
              id: filename,
              loc: { file: d.file, line: d.start.line, column: d.start.column },
            });
          }
        }
        if (errors.length > 0) throw compileError(errors, filename, seenCodes);
        return { code: replaceFlags(out.code!, ssr), map: out.map ?? null };
      },
    },
    buildEnd() {
      if (!program) return;
      const outside = [];
      for (const id of this.getModuleIds()) {
        const filename = cleanId(id);
        if (program.facts.has(filename) || isRuntime(filename) || id.startsWith("\0")) continue;
        const info = this.getModuleInfo(id);
        const imported = [...(info?.importedIds ?? []), ...(info?.dynamicallyImportedIds ?? [])].map(cleanId);
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

/** Every file under `root`, skipping `node_modules`, dot directories and the output directory. */
function scan(root: string, outDir: string): string[] {
  const files: string[] = [];
  const walk = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name === "node_modules" || entry.name.startsWith(".") || path === outDir) continue;
        walk(path);
      } else if (entry.isFile()) {
        files.push(path);
      }
    }
  };
  walk(root);
  return files;
}

/** Module inputs of the build: `build.ssr` for SSR builds, otherwise `rolldownOptions.input`. */
function inputsOf(config: ResolvedConfig): string[] {
  const ssr = config.build.ssr;
  if (typeof ssr === "string") return [resolve(config.root, ssr)];
  const input = config.build.rolldownOptions.input;
  const inputs = typeof input === "string" ? [input] : Array.isArray(input) ? input : Object.values(input ?? {});
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

