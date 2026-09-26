import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";

const here = import.meta.dirname;
const root = join(here, "..", "..");
const latestPath = join(here, "results", "latest.json");
const targets = JSON.parse(readFileSync(join(here, "targets.json"), "utf8"));
const previous = existsSync(latestPath) ? JSON.parse(readFileSync(latestPath, "utf8")).results : {};

const bytes = (n) => `${n} B`;
const delta = (current, was) =>
  was === undefined ? "" : ` (was ${bytes(was)}, ${current === was ? "same" : `${current > was ? "+" : ""}${current - was} B`})`;

const results = {};
for (const [pkg, target] of Object.entries(targets)) {
  execFileSync("pnpm", ["--filter", pkg, "build"], { cwd: root, stdio: "inherit" });
  const assets = join(root, target.dir, "dist", "assets");
  let raw = 0;
  let gzip = 0;
  for (const file of readdirSync(assets)) {
    if (!file.endsWith(".js")) continue;
    const content = readFileSync(join(assets, file));
    raw += content.length;
    gzip += gzipSync(content).length;
  }
  const key = `${pkg} js`;
  const was = previous[key];
  console.log(`${key}: raw ${bytes(raw)}${delta(raw, was?.raw)}, gzip ${bytes(gzip)}${delta(gzip, was?.gzip)}`);
  results[key] = { raw, gzip };
}

writeFileSync(latestPath, `${JSON.stringify({ schema: 1, recordedAt: new Date().toISOString(), results }, null, 2)}\n`);
console.log(`wrote ${latestPath}`);
