// Rebuilds each target, logs JS sizes, prints deltas vs results/latest.json.
// Usage: pnpm --filter @rezejs/bench-bundle-size log
import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";

const here = new URL(".", import.meta.url).pathname;
const root = join(here, "..", "..");
const dir = join(here, "results");
mkdirSync(dir, { recursive: true });
const targets = JSON.parse(readFileSync(join(here, "targets.json"), "utf8"));

let prev = {};
try {
  prev = JSON.parse(readFileSync(join(dir, "latest.json"), "utf8")).results;
} catch {
  // First run: no baseline yet.
}

const results = {};
for (const [pkg, target] of Object.entries(targets)) {
  execFileSync("pnpm", ["--filter", pkg, "build"], { cwd: root, stdio: "inherit" });
  const assets = join(root, target.dir, "dist", "assets");
  const js = readdirSync(assets).filter((f) => f.endsWith(".js"));
  let raw = 0;
  let gzip = 0;
  for (const f of js) {
    const buf = readFileSync(join(assets, f));
    raw += buf.length;
    gzip += gzipSync(buf).length;
  }
  const key = `${pkg} js`;
  const old = prev[key];
  const fmt = (n) => `${n.toLocaleString("en-US")} B`;
  const delta = (cur, was) =>
    was === undefined
      ? ""
      : `  (was ${fmt(was)}, ${cur === was ? "same" : `${cur > was ? "+" : ""}${cur - was} B`})`;
  console.log(
    `${key}: raw ${fmt(raw)}${delta(raw, old?.raw)}, gzip ${fmt(gzip)}${delta(gzip, old?.gzip)}`,
  );
  results[key] = { raw, gzip };
}

const path = join(dir, "latest.json");
writeFileSync(
  path,
  `${JSON.stringify({ schema: 1, recordedAt: new Date().toISOString(), results }, null, 2)}\n`,
);
console.log(`wrote ${path}`);
