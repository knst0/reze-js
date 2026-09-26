import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

import { compile } from "@rezejs/compiler";
import type { Environment, Plugin } from "vite";

export interface Options {
  diagnostics?: {
    /** File every diagnostic, `info` included, is appended to as one JSON line. */
    jsonl?: string;
  };
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

export default function reze(options: Options = {}): Plugin {
  const jsonl = options.diagnostics?.jsonl;
  const seenCodes = new Set<string>();
  let jsonlDirReady = false;
  let isServe = false;
  let debugNames = false;
  let hot = false;

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
    applyToEnvironment: (environment) => environment.config.consumer === "client",
    configResolved(config) {
      isServe = config.command === "serve";
      debugNames = !config.isProduction;
      hot = isServe && config.server.hmr !== false;
    },
    transform: {
      filter: { id: { include: /\.[cm]?[jt]sx?(?:$|\?)/, exclude: /\/node_modules\// } },
      handler(code, id) {
        const result = compile(code, id.replace(QueryOrHash, ""), {
          sourceMap: emitsSourceMap(this.environment.config),
          debugNames,
          hot,
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
