import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { build } from "vite";
import { afterEach, expect, test } from "vitest";

import reze, { type Options } from "../src/index";

const Flag = "__REZE_LOADING__";

type Handler = (this: unknown, code: string, id: string, options?: { ssr?: boolean }) => unknown;

interface Hooks {
  configResolved: (config: ResolvedConfig) => void;
  buildStart: (this: unknown) => Promise<void>;
  transform: { handler: Handler };
}

function hooksOf(options?: Options): Hooks {
  return reze(options) as unknown as Hooks;
}

function padded(literal: string): string {
  return `${literal}/*${" ".repeat(Flag.length - literal.length - 4)}*/`;
}

let dir: string | undefined;

afterEach(() => {
  if (dir) {
    rmSync(dir, { recursive: true, force: true });
    dir = undefined;
  }
});

function writeFlagsFixture(): { widget: string; flags: string } {
  dir = mkdtempSync(join(tmpdir(), "reze-program-"));
  const widget = join(dir, "Widget.tsx");
  const flags = join(dir, "flags.ts");
  writeFileSync(
    widget,
    `export function Widget() {\n  return <main>{${Flag} ? "deferred" : "ready"}</main>;\n}\n`,
  );
  writeFileSync(flags, `export const deferred: boolean = ${Flag};\n`);
  return { widget, flags };
}

async function scanDirectory(plugin: Hooks, root: string, entry: string): Promise<void> {
  plugin.configResolved({
    root,
    command: "build",
    build: { outDir: "dist", rolldownOptions: { input: entry } },
  } as ResolvedConfig);
  await plugin.buildStart.call({
    resolve: async (spec: string) => (spec === entry ? { id: entry } : null),
  });
}

function transformed(plugin: Hooks, file: string): string {
  const code = readFileSync(file, "utf8");
  const out = plugin.transform.handler.call({ warn: () => {} }, code, file) as {
    code: string;
    map: string | null;
  } | null;
  expect(out).not.toBeNull();
  return out!.code;
}

test("islands without hydratable throws a config error", () => {
  expect(() => reze({ islands: true })).toThrowError(/hydratable/);
  expect(() => reze({ islands: true, hydratable: false })).toThrowError(/hydratable/);
  expect(() => reze({ islands: true, hydratable: true })).not.toThrow();
  expect(() => reze({})).not.toThrow();
});

test("without a program every flag stays true", () => {
  const plugin = hooksOf();
  const { flags } = writeFlagsFixture();
  const code = transformed(plugin, flags);
  expect(code).toContain(padded("true"));
  expect(code.length).toBe(readFileSync(flags, "utf8").length);
});

test("a linked program decides the loading flag without moving columns", async () => {
  const { widget, flags } = writeFlagsFixture();
  const code = readFileSync(flags, "utf8");

  const decided = hooksOf();
  await scanDirectory(decided, dir!, widget);
  const replaced = transformed(decided, flags);
  expect(replaced).toContain(padded("false"));
  expect(replaced.length).toBe(code.length);

  const forced = hooksOf({ features: { loading: true } });
  await scanDirectory(forced, dir!, widget);
  expect(transformed(forced, flags)).toContain(padded("true"));

  const programModule = transformed(decided, widget);
  expect(programModule).toContain(padded("false"));
  expect(programModule).toContain("<main>");
});

test("verify errors fail buildEnd", async () => {
  const root = join(import.meta.dirname, "fixtures", "verify");
  const outDir = mkdtempSync(join(tmpdir(), "reze-verify-"));
  dir = outDir;
  await expect(
    build({
      root,
      logLevel: "silent",
      configFile: false,
      plugins: [reze({ program: { include: /inner/ } })],
      build: {
        outDir,
        emptyOutDir: true,
        minify: false,
        rollupOptions: {
          input: join(root, "entry.tsx"),
          preserveEntrySignatures: "strict",
          output: { format: "esm", entryFileNames: "bundle.mjs" },
        },
      },
    }),
  ).rejects.toThrow(/PROGRAM_OPEN_IMPORT/);
});
