import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";

import { analyze } from "@rezejs/compiler";

import type { ModuleFacts } from "./facts";

export interface ManifestReport {
  path: string;
  written: number;
  skipped: { file: string; reason: string }[];
}

interface PackageManifestFields {
  name?: string;
  exports?: unknown;
  reze?: { manifest?: unknown };
}

const SourceFile = /\.(?:[cm]?[jt]sx?)$/;

function rezeTargets(value: unknown): string[] {
  if (typeof value !== "object" || value === null) return [];
  if (Array.isArray(value)) return value.flatMap(rezeTargets);
  return Object.entries(value).flatMap(([key, item]) => (key === "reze" && typeof item === "string" ? [item] : rezeTargets(item)));
}

function collectSourceFiles(directory: string, files: Set<string>): void {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === "node_modules") continue;
    const path = join(directory, entry.name);
    if (entry.isDirectory()) collectSourceFiles(path, files);
    else if (SourceFile.test(entry.name) && !entry.name.endsWith(".d.ts")) files.add(path);
  }
}

/**
 * Analyzes every source under the directories of the package's `reze` export targets and writes its `reze.json`,
 * keyed by package-relative path. Modules the compiler cannot analyze are reported and left out, so importers take
 * the conservative path for them.
 */
export function writeManifest(packageDir: string): ManifestReport {
  const dir = resolve(packageDir);
  const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8")) as PackageManifestFields;
  const label = pkg.name ?? dir;
  const manifest = pkg.reze?.manifest;
  if (typeof manifest !== "string") throw new Error(`${label}: package.json declares no reze.manifest`);
  const targets = rezeTargets(pkg.exports);
  if (targets.length === 0) throw new Error(`${label}: package.json exports no "reze" condition`);

  const files = new Set<string>();
  for (const directory of new Set(targets.map((target) => dirname(resolve(dir, target))))) {
    collectSourceFiles(directory, files);
  }
  const entries: Record<string, ModuleFacts> = {};
  const skipped: ManifestReport["skipped"] = [];
  for (const file of [...files].sort()) {
    const path = relative(dir, file).split(sep).join("/");
    const result = analyze(readFileSync(file, "utf8"), file);
    if (result.facts === undefined) skipped.push({ file: path, reason: result.diagnostics.map((d) => d.code).join(", ") });
    else entries[path] = result.facts;
  }
  const path = join(dir, manifest);
  writeFileSync(path, JSON.stringify(entries, null, 2) + "\n");
  return { path, written: Object.keys(entries).length, skipped };
}
