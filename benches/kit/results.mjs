import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const latestPath = (benchDir) => join(benchDir, "results", "latest.json");

export const dependenciesOf = (dir) => JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).dependencies;

export function readResults(benchDir) {
  const path = latestPath(benchDir);
  return existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : undefined;
}

export function writeResults(benchDir, payload) {
  const path = latestPath(benchDir);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify({ schema: 1, recordedAt: new Date().toISOString(), ...payload }, null, 2)}\n`);
  console.log(`\nwrote ${path}`);
}
