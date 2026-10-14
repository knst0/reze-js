import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { measureDir } from "../../../../scripts/bench/bundle.mjs";
import { generate } from "./generate.mjs";

const here = import.meta.dirname;
const resultsPath = join(here, "..", "scaling.json");

const componentCounts = [0, 1, 5, 10, 25, 50, 100, 200];
const usageCounts = [1, 10, 50];
const usageComponents = 5;
const fitFrom = 10;
const metrics = ["raw", "gzip", "brotli"];

const cases = [
  ...componentCounts.map((components) => ({ components, usages: 1 })),
  ...usageCounts.filter((u) => u !== 1).map((usages) => ({ components: usageComponents, usages })),
];

function slope(points) {
  const n = points.length;
  const mx = points.reduce((s, [x]) => s + x, 0) / n;
  const my = points.reduce((s, [, y]) => s + y, 0) / n;
  let num = 0;
  let den = 0;
  for (const [x, y] of points) {
    num += (x - mx) * (y - my);
    den += (x - mx) ** 2;
  }
  const m = num / den;
  return { perUnit: m, intercept: my - m * mx };
}

function summarize(rows, metric) {
  const scaling = rows.filter((r) => r.usages === 1);
  const perComponent = slope(scaling.filter((r) => r.components >= fitFrom).map((r) => [r.components, r[metric]]));
  const usage = rows.filter((r) => r.components === usageComponents);
  const perUsage = slope(usage.map((r) => [r.components * r.usages, r[metric]]));
  return {
    baseline: scaling.find((r) => r.components === 0)[metric],
    perComponent: Math.round(perComponent.perUnit),
    intercept: Math.round(perComponent.intercept),
    perUsage: Math.round(perUsage.perUnit * 10) / 10,
  };
}

const workRoot = mkdtempSync(join(tmpdir(), "reze-scaling-"));
const results = [];
for (const c of cases) {
  const dir = join(workRoot, `c${c.components}-u${c.usages}`);
  for (const [file, content] of Object.entries(generate(c))) {
    mkdirSync(dirname(join(dir, file)), { recursive: true });
    writeFileSync(join(dir, file), content);
  }
  results.push({ ...c, ...(await measureDir(dir)) });
}
rmSync(workRoot, { recursive: true, force: true });

const summary = Object.fromEntries(metrics.map((metric) => [metric, summarize(results, metric)]));

console.log("\n## JS bytes by distinct components (1 usage each), usages at 5 components\n");
console.table(results.map(({ components, usages, raw, gzip, brotli }) => ({ components, usages, raw, gzip, brotli })));
console.log(`\n## Fit (components >= ${fitFrom}) per metric\n`);
console.table(metrics.map((metric) => ({ metric, ...summary[metric] })));

mkdirSync(dirname(resultsPath), { recursive: true });
writeFileSync(resultsPath, `${JSON.stringify({ schema: 1, recordedAt: new Date().toISOString(), summary, results }, null, 2)}\n`);
console.log(`wrote ${resultsPath}`);
