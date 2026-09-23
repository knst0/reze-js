import { compile } from "@rezejs/compiler";
import type { Plugin } from "vite";

export interface RezeOptions {
  /** Modules to compile. Default: `.jsx` and `.tsx` files. */
  include?: RegExp;
  /** Modules to skip. Default: anything under `node_modules`. */
  exclude?: RegExp;
  /** Module the compiled code imports its runtime from. Default: `"reze-js"`. */
  moduleName?: string;
}

/** Compiles JSX to DOM code with the Reze compiler. TypeScript is left to Vite. */
export default function reze(options: RezeOptions = {}): Plugin {
  const { moduleName } = options;
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
        const out = compile(code, filename, { moduleName });
        return out && { code: out.code, map: out.map ?? null };
      },
    },
  };
}
