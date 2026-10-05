import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

import { createLogger, createServer, type ViteDevServer } from "vite";
import { afterEach, beforeEach, expect, test } from "vitest";

import reze, { type Options } from "../src";

let root: string;
let server: ViteDevServer | undefined;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "reze-vite-plugin-"));
  mkdirSync(join(root, "src"));
  symlinkSync(join(import.meta.dirname, "..", "node_modules"), join(root, "node_modules"), "dir");
});

afterEach(async () => {
  try {
    await server?.close();
  } finally {
    server = undefined;
    rmSync(root, { recursive: true, force: true });
  }
});

async function start(options: Pick<Options, "diagnostics" | "profile">) {
  const warnings: string[] = [];
  const logger = createLogger("silent");
  logger.warn = (message) => warnings.push(message);
  server = await createServer({
    configFile: false,
    root,
    customLogger: logger,
    plugins: [reze(options)],
    server: { host: "127.0.0.1", port: 0, strictPort: true, hmr: false, watch: null },
  });
  return { vite: server, warnings };
}

test("Vite reports source locations and persists all diagnostic severities", async () => {
  const jsonl = join(root, "diagnostics", "records.jsonl");
  writeFileSync(join(root, "src", "Warn.tsx"), "export function App(props){\nreturn <div children={props.children}>nested</div>;\n}");
  writeFileSync(
    join(root, "src", "Info.tsx"),
    'import {$signal} from "reze-js"; export function App(){let count=$signal(1);return <p>{count}</p>}',
  );
  writeFileSync(join(root, "src", "Bad.tsx"), "export function Bad(){\nreturn <div>;\n}");
  const { vite, warnings } = await start({ diagnostics: { jsonl } });
  await vite.transformRequest("/src/Warn.tsx");
  await vite.transformRequest("/src/Info.tsx");
  await expect(vite.transformRequest("/src/Bad.tsx")).rejects.toMatchObject({
    plugin: "reze-js",
    loc: { file: join(root, "src", "Bad.tsx"), line: 3, column: 0 },
    diagnostics: [expect.objectContaining({ code: "PARSE_ERROR", severity: "error" })],
  });
  const records = readFileSync(jsonl, "utf8")
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line));
  expect(records.map((record) => [record.code, record.severity, basename(record.file), record.start.line])).toEqual([
    ["CHILDREN_PROP_IGNORED", "warn", "Warn.tsx", 2],
    ["SIGNAL_FOLDED", "info", "Info.tsx", 1],
    ["PARSE_ERROR", "error", "Bad.tsx", 3],
  ]);
  expect(warnings.filter((message) => message.includes("CHILDREN_PROP_IGNORED"))).toHaveLength(1);
  expect(warnings.some((message) => message.includes("SIGNAL_FOLDED"))).toBe(false);
});

test("profiling HTTP admits transformed files and accumulates their sessions", async () => {
  const profileDir = join(root, "profiles");
  const file = join(root, "src", "App.tsx");
  writeFileSync(file, "export function App(){return <p>profile</p>}");
  const { vite } = await start({ profile: { dir: profileDir } });
  await vite.listen();
  const endpoint = new URL("/__reze/profile", vite.resolvedUrls!.local[0]!);
  const post = (body: string) => fetch(endpoint, { method: "POST", headers: { "content-type": "application/json" }, body });
  expect((await post("{oops")).status).toBe(400);
  const tree = { v: 1, components: [{ component: "App", file, mounts: 1, props: 2, reruns: 0, writes: 3 }] };
  expect((await post(JSON.stringify(tree))).status).toBe(200);
  expect(existsSync(profileDir) ? readdirSync(profileDir) : []).toEqual([]);
  await vite.transformRequest("/src/App.tsx");
  expect((await post(JSON.stringify(tree))).status).toBe(200);
  expect((await post(JSON.stringify(tree))).status).toBe(200);
  const records = readdirSync(profileDir).map((name) => JSON.parse(readFileSync(join(profileDir, name), "utf8")));
  expect(records).toEqual([
    expect.objectContaining({
      v: 1,
      file,
      components: [{ component: "App", file, mounts: 2, props: 2, reruns: 0, writes: 6 }],
    }),
  ]);
});
