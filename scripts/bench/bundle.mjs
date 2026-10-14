import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, symlinkSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { build } from "vite";

import { measure, sumSizes } from "./sizes.mjs";

const repoRoot = resolve(import.meta.dirname, "..", "..");
const packagesDir = join(repoRoot, "packages");
const workspaceDirs = new Map(
  readdirSync(packagesDir)
    .filter((dir) => existsSync(join(packagesDir, dir, "package.json")))
    .map((dir) => [JSON.parse(readFileSync(join(packagesDir, dir, "package.json"), "utf8")).name, join(packagesDir, dir)]),
);
const shadowDir = join(repoRoot, "node_modules", ".bench-workspace");
for (const [name, dir] of workspaceDirs) {
  const link = join(shadowDir, name);
  mkdirSync(dirname(link), { recursive: true });
  rmSync(link, { force: true });
  symlinkSync(dir, link, "dir");
}

function exportTarget(value) {
  if (typeof value === "string") return value;
  for (const [condition, target] of Object.entries(value)) {
    if (condition !== "import" && condition !== "default") continue;
    const resolved = exportTarget(target);
    if (resolved !== undefined) return resolved;
  }
  return undefined;
}

function resolveWorkspace(specifier) {
  const match = /^(@[^/]+\/[^/]+|[^@/][^/]*)(\/.*)?$/.exec(specifier);
  const dir = match && workspaceDirs.get(match[1]);
  if (!dir) return undefined;
  const { exports } = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  const target = exports?.[match[2] ? `.${match[2]}` : "."];
  const file = target === undefined ? undefined : exportTarget(target);
  return file === undefined ? undefined : join(shadowDir, match[1], file);
}

const workspacePlugin = {
  name: "rezejs:bench-workspace",
  enforce: "pre",
  resolveId: resolveWorkspace,
};

const { default: reze } = await import(pathToFileURL(resolveWorkspace("@rezejs/vite-plugin")).href);

export async function measureDir(dir) {
  const entry = readdirSync(dir).find((file) => /^main\.[jt]sx?$/.test(file));
  const output = await build({
    root: dir,
    configFile: false,
    logLevel: "silent",
    resolve: { preserveSymlinks: true },
    plugins: [workspacePlugin, reze()],
    build: {
      write: false,
      modulePreload: { polyfill: false },
      rollupOptions: { input: join(dir, entry), preserveEntrySignatures: "strict" },
    },
  });
  const chunks = [output]
    .flat()
    .flatMap((result) => result.output)
    .filter((chunk) => chunk.type === "chunk");
  return sumSizes(chunks.map((chunk) => measure(chunk.code)));
}
