import { execFile, execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { promisify } from "node:util";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

import { frameworks, generate } from "./generate.mjs";

const run = promisify(execFile);
const here = import.meta.dirname;
const root = join(here, "..", "..");

const componentCounts = [0, 1, 5, 10, 25, 50, 100, 200];
const usageCounts = [1, 10, 50];
const usageComponents = 5;
const fitFrom = 10;

const cases = [
  ...componentCounts.map((components) => ({ components, usages: 1 })),
  ...usageCounts.filter((u) => u !== 1).map((usages) => ({ components: usageComponents, usages })),
];

const brotli = (buf) => brotliCompressSync(buf, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length;

function measure(dist) {
  const assets = join(dist, "assets");
  let raw = 0;
  let gzip = 0;
  let br = 0;
  for (const file of readdirSync(assets)) {
    if (!file.endsWith(".js")) continue;
    const content = readFileSync(join(assets, file));
    raw += content.length;
    gzip += gzipSync(content, { level: 9 }).length;
    br += brotli(content);
  }
  return { raw, gzip, brotli: br };
}

async function benchFramework(framework) {
  const dir = join(here, "frameworks", framework);
  const src = join(dir, "src");
  const rows = [];
  for (const c of cases) {
    rmSync(src, { recursive: true, force: true });
    mkdirSync(src, { recursive: true });
    for (const [file, content] of Object.entries(generate(framework, c))) writeFileSync(join(src, file), content);
    try {
      await run("pnpm", ["exec", "vite", "build", "--logLevel", "error"], { cwd: dir, maxBuffer: 1 << 26 });
    } catch (error) {
      throw new Error(`${framework} ${JSON.stringify(c)} failed:\n${error.stdout}\n${error.stderr}`);
    }
    rows.push({ ...c, ...measure(join(dir, "dist")) });
  }
  return rows;
}

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

function crossover(a, b) {
  if (a.perComponent === b.perComponent) return null;
  const n = (b.intercept - a.intercept) / (a.perComponent - b.perComponent);
  return n > 0 ? Math.round(n) : null;
}

execFileSync("pnpm", ["build"], { cwd: root, stdio: "inherit" });

const results = Object.fromEntries(await Promise.all(frameworks.map(async (framework) => [framework, await benchFramework(framework)])));

const versions = Object.fromEntries(
  frameworks.map((framework) => [
    framework,
    JSON.parse(readFileSync(join(here, "frameworks", framework, "package.json"), "utf8")).dependencies,
  ]),
);

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

const out = join(here, "results", "latest.json");
mkdirSync(join(here, "results"), { recursive: true });
writeFileSync(out, `${JSON.stringify({ schema: 1, recordedAt: new Date().toISOString(), versions, summary, results }, null, 2)}\n`);
console.log(`\nwrote ${out}`);
