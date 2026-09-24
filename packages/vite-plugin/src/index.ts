import { compile } from "@rezejs/compiler";
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
}

/** Compiles JSX to DOM code with the Reze compiler. TypeScript is left to Vite. */
export default function reze(options: RezeOptions = {}): Plugin {
  const { moduleName, sourcemap } = options;
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
      handler(code, id) {
        const filename = id.replace(/[?#].*$/, "");
        let out;
        try {
          out = compile(code, filename, { moduleName, sourceMap: sourcemap });
        } catch (e) {
          throw withLoc(e, filename, code);
        }
        if (out?.warnings) {
          for (const w of out.warnings) {
            this.warn({
              message: w.message,
              id: filename,
              loc: { file: filename, line: w.line, column: w.column },
            });
          }
        }
        return out && { code: out.code, map: out.map ?? null };
      },
    },
  };
}

/** Rollup-style location: what the Vite overlay jumps to. */
export interface RezeErrorLocation {
  file: string;
  line: number;
  column: number;
}

/**
 * Gives a compiler failure the `loc`/`frame`/`id` the Vite overlay needs (D01).
 * The native binding reports `{filename}:{line}:{column}: {message}` (1-based);
 * anything not matching is rethrown untouched.
 */
export function withLoc(error: unknown, filename: string, code: string): unknown {
  if (error instanceof Error) {
    const match = /^(.+?):(\d+):(\d+): ([\s\S]*)$/.exec(error.message);
    if (match) {
      const line = Number(match[2]);
      const column = Number(match[3]);
      const err = error as Error & { loc?: RezeErrorLocation; frame?: string; id?: string };
      err.loc = { file: filename, line, column };
      err.id = filename;
      err.frame = codeFrame(code, line, column);
      return err;
    }
  }
  return error;
}

/** Three lines of context with a caret row, in the `@babel/code-frame` shape. */
function codeFrame(code: string, line: number, column: number): string {
  const lines = code.split("\n");
  const first = Math.max(1, line - 2);
  const last = Math.min(lines.length, line + 2);
  const width = String(last).length;
  const out: string[] = [];
  for (let n = first; n <= last; n++) {
    const gutter = n === line ? ">" : " ";
    out.push(`${gutter} ${String(n).padStart(width, " ")} | ${lines[n - 1]}`);
    if (n === line) {
      out.push(`  ${" ".repeat(width)} | ${" ".repeat(Math.max(0, column - 1))}^`);
    }
  }
  return out.join("\n");
}
