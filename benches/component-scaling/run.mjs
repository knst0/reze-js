import { join } from "node:path";

import { buildPackages, viteBuild, writeSources } from "../kit/build.mjs";
import { dependenciesOf, writeResults } from "../kit/results.mjs";
import { totalSize } from "../kit/sizes.mjs";
import { slope } from "../kit/stats.mjs";
import { frameworks, generate } from "./generate.mjs";

const here = import.meta.dirname;

const componentCounts = [0, 1, 5, 10, 25, 50, 100, 200];
const usageCounts = [1, 10, 50];
const usageComponents = 5;
const fitFrom = 10;

const cases = [
  ...componentCounts.map((components) => ({ components, usages: 1 })),
  ...usageCounts.filter((u) => u !== 1).map((usages) => ({ components: usageComponents, usages })),
];

async function benchFramework(framework) {
  const dir = join(here, "frameworks", framework);
  const rows = [];
  for (const c of cases) {
    writeSources(join(dir, "src"), generate(framework, c));
    await viteBuild(dir, `${framework} ${JSON.stringify(c)}`);
    rows.push({ ...c, ...totalSize(join(dir, "dist")) });
  }
  return rows;
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

function crossover(a, b) {
  if (a.perComponent === b.perComponent) return null;
  const n = (b.intercept - a.intercept) / (a.perComponent - b.perComponent);
  return n > 0 ? Math.round(n) : null;
}

buildPackages();

const results = Object.fromEntries(await Promise.all(frameworks.map(async (framework) => [framework, await benchFramework(framework)])));

const versions = Object.fromEntries(frameworks.map((framework) => [framework, dependenciesOf(join(here, "frameworks", framework))]));

const summary = {};
for (const metric of ["raw", "gzip", "brotli"]) {
  summary[metric] = Object.fromEntries(frameworks.map((f) => [f, summarize(results[f], metric)]));
}

const pad = (s, n) => String(s).padStart(n);
for (const metric of ["raw", "gzip", "brotli"]) {
  console.log(`\n## ${metric} JS bytes by distinct components (1 usage each)\n`);
  console.log(`| framework | ${componentCounts.map((n) => pad(n, 7)).join(" | ")} |`);
  console.log(`| --- | ${componentCounts.map(() => "---:").join(" | ")} |`);
  for (const f of frameworks) {
    const byCount = results[f].filter((r) => r.usages === 1);
    console.log(`| ${f} | ${componentCounts.map((n) => pad(byCount.find((r) => r.components === n)[metric], 7)).join(" | ")} |`);
  }
  console.log(`\n## ${metric} fit (components >= ${fitFrom}), usages at ${usageComponents} components\n`);
  console.log("| framework | baseline | B/component | B/usage | crossover with reze (components) |");
  console.log("| --- | ---: | ---: | ---: | ---: |");
  const reze = summary[metric].reze;
  for (const f of frameworks) {
    const s = summary[metric][f];
    const x = f === "reze" ? "" : (crossover(reze, s) ?? "none");
    console.log(`| ${f} | ${s.baseline} | ${s.perComponent} | ${s.perUsage} | ${x} |`);
  }
}

writeResults(here, { versions, summary, results });
