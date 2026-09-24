import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

import { compile, type Diagnostic } from "@rezejs/compiler";
import type { Plugin } from "vite";

export interface RezeOptions {
  /** Modules to compile. Default: `.jsx` and `.tsx` files. */
  include?: RegExp;
  /** Modules to skip. Default: anything under `node_modules`. */
  exclude?: RegExp;
  /** Module the compiled code imports its runtime from. Default: `"reze-js"`. */
  moduleName?: string;
  /**
   * Emit source maps. Default: `true`. `false` skips the map builder in the
   * native compiler (C27), which measurably speeds up large files.
   */
  sourcemap?: boolean;
  /** Enables the compiler optimizations gated behind `optimize` (constant signals, dead JSX branches). */
  optimize?: boolean;
  /**
   * Server-side rendering with hydration. SSR transforms always compile for the `server` target
   * (HTML strings for `renderToString`); with `hydratable`, browser transforms compile for
   * `hydrate` (adopting that HTML with `hydrate`) instead of `client`. Default: `false`.
   */
  hydratable?: boolean;
  diagnostics?: {
    /** File every diagnostic (all severities, including `info`) is appended to as one JSON line. */
    jsonl?: string;
  };
}

/** Rollup-style location: what the Vite overlay jumps to. */
export interface RezeErrorLocation {
  file: string;
  line: number;
  column: number;
}

/** What the transform throws when the compiler reports errors: the Vite overlay reads `loc`/`frame`/`id`. */
export interface RezeCompileError extends Error {
  id: string;
  loc: RezeErrorLocation;
  frame: string;
  plugin: "rezejs";
  diagnostics: Diagnostic[];
}

const SkillGuide = "node_modules/@rezejs/compiler/skills/compiler-diagnostics/SKILL.md";

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

/** Compiles JSX to DOM code with the Reze compiler. TypeScript is left to Vite. */
export default function reze(options: RezeOptions = {}): Plugin {
  const { moduleName, sourcemap, optimize, hydratable } = options;
  const jsonl = options.diagnostics?.jsonl;
  const seenCodes = new Set<string>();
  let jsonlDirReady = false;

  function record(diagnostics: Diagnostic[]): void {
    if (!jsonl || diagnostics.length === 0) return;
    if (!jsonlDirReady) {
      mkdirSync(dirname(jsonl), { recursive: true });
      jsonlDirReady = true;
    }
    appendFileSync(jsonl, diagnostics.map((d) => JSON.stringify(d) + "\n").join(""));
  }

  return {
    name: "rezejs",
    enforce: "pre",
    transform: {
      filter: {
        id: {
          include: options.include ?? /\.[jt]sx(?:$|\?)/,
          exclude: options.exclude ?? /\/node_modules\//,
        },
      },
      handler(code, id, transformOptions) {
        const filename = id.replace(/[?#].*$/, "");
        const target = transformOptions?.ssr ? "server" : hydratable ? "hydrate" : "client";
        const out = compile(code, filename, { moduleName, sourceMap: sourcemap, optimize, target });
        if (out === null) return null;
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
        return { code: out.code!, map: out.map ?? null };
      },
    },
  };
}

function compileError(errors: Diagnostic[], id: string, seenCodes: Set<string>): RezeCompileError {
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
