// Compacts results/history.jsonl into results/latest.json and prints a table
// with deltas vs the previous run. Usage: node ./scripts/merge.mjs [run-id]
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const here = new URL(".", import.meta.url).pathname;
const dir = join(here, "..", "results");
mkdirSync(dir, { recursive: true });

const lines = readFileSync(join(dir, "history.jsonl"), "utf8")
  .split("\n")
  .filter(Boolean)
  .map((l) => JSON.parse(l));

const run = process.argv[2] ?? lines.at(-1).run;
const rows = lines.filter((l) => l.run === run);
if (rows.length === 0) throw new Error(`no results for run ${run}`);

let prev = {};
try {
  prev = JSON.parse(readFileSync(join(dir, "latest.json"), "utf8")).results;
} catch {
  // First run: no baseline yet.
}

const results = {};
for (const r of rows) results[`${r.group} / ${r.name}`] = { hz: r.hz, meanMs: r.meanMs, p50Ms: r.p50Ms };

const width = Math.max(...Object.keys(results).map((n) => n.length));
for (const [name, cur] of Object.entries(results)) {
  const old = prev[name];
  const delta = old ? `  (was ${old.hz.toLocaleString("en-US", { maximumFractionDigits: 0 })} Hz, ${(cur.hz / old.hz >= 1 ? "+" : "") + ((cur.hz / old.hz - 1) * 100).toFixed(1)}%)` : "  (first run)";
  console.log(`${name.padEnd(width)}  ${cur.hz.toLocaleString("en-US", { maximumFractionDigits: 0 }).padStart(14)} Hz  p50 ${cur.p50Ms.toFixed(4)} ms${delta}`);
}

const path = join(dir, "latest.json");
writeFileSync(path, `${JSON.stringify({ schema: 1, recordedAt: run, results }, null, 2)}\n`);
console.log(`wrote ${path}`);
