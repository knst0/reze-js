import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { Diagnostic } from "@rezejs/compiler";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import reze, { formatDiagnostic, type RezeCompileError, type RezeOptions } from "../src/index";

const compile = vi.hoisted(() => vi.fn());
vi.mock("@rezejs/compiler", () => ({ compile }));

const Guide = "node_modules/@rezejs/compiler/skills/compiler-diagnostics/SKILL.md";

function diagnostic(overrides: Partial<Diagnostic> = {}): Diagnostic {
  const code = overrides.code ?? "CLASS_ALIAS";
  return {
    code,
    severity: "warn",
    message: `[${code}] \`classList\` is a legacy alias.`,
    file: "src/Counter.tsx",
    start: { offset: 120, line: 8, column: 14 },
    end: { offset: 129, line: 8, column: 23 },
    path: ["<Counter>", "output"],
    labels: [],
    fixes: [],
    data: {},
    docs: `https://github.com/knst0/reze-js/blob/main/packages/compiler/skills/compiler-diagnostics/SKILL.md#${code.toLowerCase()}`,
    rendered: `[${code}] \`classList\` is a legacy alias.\n  in <Counter> › output\n  at src/Counter.tsx:8:15`,
    ...overrides,
  };
}

function compiled(diagnostics: Diagnostic[]) {
  const failed = diagnostics.some((d) => d.severity === "error");
  return { code: failed ? null : "compiled();", map: failed ? null : "{}", diagnostics };
}

type Handler = (this: unknown, code: string, id: string, options?: { ssr?: boolean }) => unknown;

function pluginWith(options?: RezeOptions) {
  const plugin = reze(options) as unknown as { transform: { handler: Handler } };
  const warnings: { message: string; id: string; loc: unknown }[] = [];
  const ctx = {
    warn(w: { message: string; id: string; loc: unknown }) {
      warnings.push(w);
    },
  };
  const run = (id = "src/Counter.tsx?v=1") => plugin.transform.handler.call(ctx, "<i />", id);
  return { run, warnings };
}

function thrownBy(run: () => unknown): RezeCompileError {
  try {
    run();
  } catch (e) {
    return e as RezeCompileError;
  }
  return expect.unreachable("the transform throws");
}

beforeEach(() => {
  compile.mockReset();
});

test("formatDiagnostic appends the repair guide only on the first occurrence of a code", () => {
  const seen = new Set<string>();
  const d = diagnostic();
  expect(formatDiagnostic(d, seen)).toBe(
    `${d.rendered}\n  repair guide: ${Guide}#class_alias\n                ${d.docs}`,
  );
  expect(formatDiagnostic(d, seen)).toBe(d.rendered);
  const other = diagnostic({ code: "KEY_ON_ELEMENT" });
  expect(formatDiagnostic(other, seen)).toContain(`${Guide}#key_on_element`);
});

test("files without JSX pass through as null", () => {
  compile.mockReturnValue(null);
  expect(pluginWith().run("a.ts")).toBeNull();
});

test("options reach the compiler and the query is stripped from the filename", () => {
  compile.mockReturnValue(compiled([]));
  const out = reze({ moduleName: "my-runtime", sourcemap: false, optimize: true }) as unknown as {
    transform: { handler: Handler };
  };
  expect(out.transform.handler.call({}, "<i />", "src/A.tsx?v=1")).toEqual({
    code: "compiled();",
    map: "{}",
  });
  expect(compile).toHaveBeenCalledWith("<i />", "src/A.tsx", {
    moduleName: "my-runtime",
    sourceMap: false,
    optimize: true,
    target: "client",
  });
});

test("SSR transforms compile for the server; hydratable browser transforms for hydrate", () => {
  compile.mockReturnValue(compiled([]));
  const targets = (options: RezeOptions) => {
    const { handler } = (reze(options) as unknown as { transform: { handler: Handler } }).transform;
    handler.call({}, "<i />", "src/A.tsx", { ssr: true });
    handler.call({}, "<i />", "src/A.tsx", { ssr: false });
    return compile.mock.calls.splice(0).map((call) => call[2].target);
  };
  expect(targets({})).toEqual(["server", "client"]);
  expect(targets({ hydratable: true })).toEqual(["server", "hydrate"]);
});

test("warnings become Vite warnings with loc; the footer appears once per code per plugin", () => {
  const warn = diagnostic();
  compile.mockReturnValue(compiled([warn, warn]));
  const { run, warnings } = pluginWith();
  run();
  run();
  expect(warnings).toHaveLength(4);
  expect(warnings[0]).toEqual({
    message: expect.stringContaining(`repair guide: ${Guide}#class_alias`),
    id: "src/Counter.tsx",
    loc: { file: "src/Counter.tsx", line: 8, column: 14 },
  });
  expect(warnings.slice(1).map((w) => w.message)).toEqual([warn.rendered, warn.rendered, warn.rendered]);
});

test("a fresh plugin instance prints the footer again", () => {
  compile.mockReturnValue(compiled([diagnostic()]));
  pluginWith().run();
  const { run, warnings } = pluginWith();
  run();
  expect(warnings[0].message).toContain("repair guide:");
});

test("info diagnostics are never printed", () => {
  compile.mockReturnValue(compiled([diagnostic({ code: "SIGNAL_FOLDED", severity: "info" })]));
  const { run, warnings } = pluginWith();
  expect(run()).toEqual({ code: "compiled();", map: "{}" });
  expect(warnings).toEqual([]);
});

test("errors throw with the overlay's loc, frame and id, carrying the error diagnostics", () => {
  const parse = diagnostic({
    code: "PARSE_ERROR",
    severity: "error",
    file: "src/Broken.tsx",
    start: { offset: 23, line: 2, column: 10 },
    rendered: "[PARSE_ERROR] Unexpected token.\n  at src/Broken.tsx:2:11",
  });
  const second = diagnostic({
    code: "PARSE_ERROR",
    severity: "error",
    file: "src/Broken.tsx",
    start: { offset: 40, line: 3, column: 2 },
    rendered: "[PARSE_ERROR] Expected `}`.\n  at src/Broken.tsx:3:3",
  });
  const warn = diagnostic({ file: "src/Broken.tsx" });
  compile.mockReturnValue(compiled([warn, parse, second]));
  const { run, warnings } = pluginWith();
  const err = thrownBy(() => run("src/Broken.tsx"));
  expect(err).toBeInstanceOf(Error);
  expect(err.message).toContain(parse.rendered);
  expect(err.message).toContain(`${Guide}#parse_error`);
  expect(err.message).toContain(second.rendered);
  expect(err.message.indexOf(parse.rendered)).toBeLessThan(err.message.indexOf(second.rendered));
  expect(err.loc).toEqual({ file: "src/Broken.tsx", line: 2, column: 10 });
  expect(err.id).toBe("src/Broken.tsx");
  expect(err.frame).toBe(parse.rendered);
  expect(err.plugin).toBe("rezejs");
  expect(err.diagnostics).toEqual([parse, second]);
  expect(warnings).toHaveLength(1);
});

let dir: string | undefined;

afterEach(() => {
  if (dir) rmSync(dir, { recursive: true, force: true });
  dir = undefined;
});

test("diagnostics.jsonl appends every diagnostic, all severities, as one JSON line each", () => {
  dir = mkdtempSync(join(tmpdir(), "reze-jsonl-"));
  const jsonl = join(dir, "nested", "diagnostics.jsonl");
  const warn = diagnostic();
  const info = diagnostic({ code: "SIGNAL_FOLDED", severity: "info" });
  const error = diagnostic({ code: "PARSE_ERROR", severity: "error" });
  const { run } = pluginWith({ diagnostics: { jsonl } });
  compile.mockReturnValueOnce(compiled([warn, info]));
  run();
  compile.mockReturnValueOnce(compiled([error]));
  thrownBy(run);
  const lines = readFileSync(jsonl, "utf8").split("\n");
  expect(lines.pop()).toBe("");
  expect(lines.map((line) => JSON.parse(line))).toEqual([warn, info, error]);
});
