import { join } from "node:path";
import { parseArgs } from "node:util";

import { chromium } from "playwright";

import { assertClean, heapBytes, openPage, taskSeconds } from "../kit/browser.mjs";
import { buildPackages, viteBuild } from "../kit/build.mjs";
import { selectNames } from "../kit/cli.mjs";
import { kib, ms, ratio, signed } from "../kit/format.mjs";
import { dependenciesOf, readResults, writeResults } from "../kit/results.mjs";
import { origin, serve } from "../kit/serve.mjs";
import { totalSize } from "../kit/sizes.mjs";
import { median } from "../kit/stats.mjs";

export const lanes = [
  "reze",
  "octane",
  "react-hooks",
  "react-compiler",
  "react-zustand",
  "react-redux",
  "react-mobx",
  "react-valtio",
  "solid",
  "svelte",
  "vue",
  "vue-vapor",
];

const here = import.meta.dirname;
const upstream = { repo: "krausest/js-framework-benchmark", commit: "f2df01a8679de05225c32714ca8cecbea3d78c5d" };
const ops = ["create", "replace", "update", "select", "swap", "remove", "createLots", "append", "clear"];
const memoryCycles = 5;

const { values: args } = parseArgs({
  options: {
    runs: { type: "string", default: "3" },
    iterations: { type: "string", default: "10" },
    warmup: { type: "string", default: "2" },
    cpu: { type: "string", default: "1" },
    only: { type: "string" },
    "skip-build": { type: "boolean", default: false },
  },
});

const runs = Number(args.runs);
const iterations = Number(args.iterations);
const warmup = Number(args.warmup);
const cpuThrottle = Number(args.cpu);
const selected = selectNames(lanes, args.only, "lane");

const laneDir = (lane) => join(here, "frameworks", lane);
const distDir = (lane) => join(laneDir(lane), "dist");
const openBenchPage = (browser) => openPage(browser, { initScript: join(here, "harness.js"), cpuThrottle });

async function measurePerf(browser, url, lane) {
  const { context, page, cdp, errors } = await openBenchPage(browser);
  await page.goto(url);
  const bootMs = await page.evaluate(() => window.__bench.ready);
  const samples = {};
  for (const op of ops) {
    for (let i = 0; i < warmup; i++) {
      await page.evaluate((o) => window.__bench.prepare(o), op);
      await page.evaluate((o) => window.__bench.run(o), op);
    }
    samples[op] = [];
    for (let i = 0; i < iterations; i++) {
      await page.evaluate((o) => window.__bench.prepare(o), op);
      const cpuBefore = await taskSeconds(cdp);
      const commitMs = await page.evaluate((o) => window.__bench.run(o), op);
      const cpuAfter = await taskSeconds(cdp);
      samples[op].push({ commitMs, cpuMs: (cpuAfter - cpuBefore) * 1000 });
    }
  }
  assertClean(errors, lane);
  await context.close();
  return { bootMs, samples };
}

async function measureMemory(browser, url, lane) {
  const { context, page, cdp, errors } = await openBenchPage(browser);
  await page.goto(url);
  await page.evaluate(() => window.__bench.ready);
  const readyHeap = await heapBytes(cdp);
  await page.evaluate(() => window.__bench.prepare("replace"));
  const rowsHeap = await heapBytes(cdp);
  await page.evaluate((n) => window.__bench.cycle(n), memoryCycles);
  const cycledHeap = await heapBytes(cdp);
  assertClean(errors, lane);
  await context.close();
  return { ready: readyHeap, rows: rowsHeap - readyHeap, retained: cycledHeap - readyHeap };
}

function summarize(lane, perfRuns, memoryRuns, bundle) {
  const summaryOps = {};
  for (const op of ops) {
    const pooled = perfRuns.flatMap((r) => r.samples[op]);
    summaryOps[op] = { commitMs: median(pooled.map((s) => s.commitMs)), cpuMs: median(pooled.map((s) => s.cpuMs)) };
  }
  const pick = (field) => median(memoryRuns.map((m) => m[field]));
  return {
    lane,
    bundle,
    bootMs: median(perfRuns.map((r) => r.bootMs)),
    ops: summaryOps,
    memory: { ready: pick("ready"), rows: pick("rows"), retained: pick("retained") },
  };
}

function table(title, headers, summaries, cells) {
  console.log(`\n## ${title}\n`);
  console.log(`| lane | ${headers.join(" | ")} |`);
  console.log(`| --- | ${headers.map(() => "---:").join(" | ")} |`);
  for (const s of summaries) console.log(`| ${s.lane} | ${cells(s).join(" | ")} |`);
}

function report(summaries, previous) {
  const reze = summaries.find((s) => s.lane === "reze");
  const rel = (s, f) => (reze === undefined || s === reze ? "" : ` (${ratio(f(s), f(reze))})`);
  const pooled = `median of ${runs}×${iterations}; CPU throttle ${cpuThrottle}×`;
  table(`Operations, commit ms (${pooled})`, ops, summaries, (s) =>
    ops.map((op) => ms(s.ops[op].commitMs) + rel(s, (x) => x.ops[op].commitMs)),
  );
  table(`Operations, CPU ms (${pooled})`, ops, summaries, (s) => ops.map((op) => ms(s.ops[op].cpuMs) + rel(s, (x) => x.ops[op].cpuMs)));
  table("Bundle and boot", ["gzip KiB", "brotli KiB", "boot ms"], summaries, (s) => [
    kib(s.bundle.gzip) + rel(s, (x) => x.bundle.gzip),
    kib(s.bundle.brotli) + rel(s, (x) => x.bundle.brotli),
    ms(s.bootMs) + rel(s, (x) => x.bootMs),
  ]);
  table(
    `Memory, KiB after forced GC (median of ${runs})`,
    ["ready heap", "1k rows cost", `retained after ${memoryCycles} create/clear cycles`],
    summaries,
    (s) => [
      kib(s.memory.ready) + rel(s, (x) => x.memory.ready),
      kib(s.memory.rows) + rel(s, (x) => x.memory.rows),
      signed(s.memory.retained, kib),
    ],
  );

  const before = previous?.summaries?.find((s) => s.lane === "reze");
  if (reze === undefined || before === undefined) return;
  const rows = [
    ...ops.map((op) => ({ label: `${op} commit ms`, pick: (s) => s.ops[op].commitMs, format: ms })),
    { label: "bundle gzip KiB", pick: (s) => s.bundle.gzip, format: kib },
    { label: "ready heap KiB", pick: (s) => s.memory.ready, format: kib },
    { label: "1k rows cost KiB", pick: (s) => s.memory.rows, format: kib },
    { label: "retained KiB", pick: (s) => s.memory.retained, format: kib },
  ];
  const percent = (value) => `${value.toFixed(1)}%`;
  console.log(`\n## reze vs previous run (${previous.recordedAt})\n`);
  console.log("| metric | previous | current | Δ |");
  console.log("| --- | ---: | ---: | ---: |");
  for (const { label, pick, format } of rows) {
    const was = pick(before);
    const now = pick(reze);
    const delta = was === 0 ? "" : signed(((now - was) / Math.abs(was)) * 100, percent);
    console.log(`| ${label} | ${format(was)} | ${format(now)} | ${delta} |`);
  }
}

if (!args["skip-build"]) {
  buildPackages();
  await Promise.all(selected.map((lane) => viteBuild(laneDir(lane), lane)));
}

const servers = {};
const bundles = {};
for (const lane of selected) {
  servers[lane] = await serve(distDir(lane));
  bundles[lane] = totalSize(distDir(lane));
}

const browser = await chromium.launch();
const browserVersion = browser.version();
const perfRuns = Object.fromEntries(selected.map((lane) => [lane, []]));
const memoryRuns = Object.fromEntries(selected.map((lane) => [lane, []]));
try {
  for (let r = 0; r < runs; r++) {
    const order = selected.map((_, i) => selected[(i + r) % selected.length]);
    for (const lane of order) {
      const url = origin(servers[lane]);
      perfRuns[lane].push(await measurePerf(browser, url, lane));
      memoryRuns[lane].push(await measureMemory(browser, url, lane));
      console.error(`run ${r + 1}/${runs}: ${lane} done`);
    }
  }
} finally {
  await browser.close();
  for (const server of Object.values(servers)) server.close();
}

const summaries = selected.map((lane) => summarize(lane, perfRuns[lane], memoryRuns[lane], bundles[lane]));
const previous = readResults(here);
report(summaries, previous);

writeResults(here, {
  browser: browserVersion,
  options: { runs, iterations, warmup, cpuThrottle },
  upstream,
  versions: Object.fromEntries(selected.map((lane) => [lane, dependenciesOf(laneDir(lane))])),
  summaries,
});
