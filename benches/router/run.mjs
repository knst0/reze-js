import { join } from "node:path";
import { parseArgs } from "node:util";

import { chromium } from "playwright";

import { assertClean, heapBytes, openPage as openPageWith, taskSeconds } from "../kit/browser.mjs";
import { buildPackages, viteBuild, writeSources } from "../kit/build.mjs";
import { selectNames } from "../kit/cli.mjs";
import { kib, ratio, signed, us } from "../kit/format.mjs";
import { dependenciesOf, writeResults } from "../kit/results.mjs";
import { origin, serve } from "../kit/serve.mjs";
import { assetSizes, diffSizes, sumSizes } from "../kit/sizes.mjs";
import { median } from "../kit/stats.mjs";
import { generate, routers, variants } from "./generate.mjs";

const here = import.meta.dirname;

const { values: args } = parseArgs({
  options: {
    runs: { type: "string", default: "3" },
    batches: { type: "string", default: "10" },
    navs: { type: "string", default: "100" },
    cpu: { type: "string", default: "1" },
    sections: { type: "string", default: "20" },
    only: { type: "string" },
    "skip-build": { type: "boolean", default: false },
  },
});

const runs = Number(args.runs);
const batches = Number(args.batches);
const navsPerBatch = Number(args.navs) & ~1;
const cpuThrottle = Number(args.cpu);
const sections = Number(args.sections);
const selected = selectNames(routers, args.only, "router");
const scenarios = ["param", "swap", "pop"];
const retainedNavs = 1000;

const appDir = (router) => join(here, "apps", router);
const distDir = (router, variant) => join(appDir(router), "dist", variant);

async function build(router) {
  for (const variant of variants) {
    writeSources(join(appDir(router), "src", variant), generate(router, variant, sections));
    await viteBuild(appDir(router), `${router} ${variant}`, { BENCH_VARIANT: variant });
  }
}

function openPage(browser) {
  return openPageWith(browser, { initScript: join(here, "harness.js"), cpuThrottle });
}

async function measurePerf(browser, url, router) {
  const { context, page, cdp, errors } = await openPage(browser);
  await page.goto(`${url}/users/1`);
  const bootMs = await page.evaluate(() => window.__bench.boot);
  await page.evaluate(() => window.__bench.user(1));
  const samples = {};
  for (const scenario of scenarios) {
    await page.evaluate(([s, n]) => window.__bench.run(s, n), [scenario, navsPerBatch]);
    samples[scenario] = [];
    for (let b = 0; b < batches; b++) {
      const cpuBefore = await taskSeconds(cdp);
      const elapsedMs = await page.evaluate(([s, n]) => window.__bench.run(s, n), [scenario, navsPerBatch]);
      const cpuAfter = await taskSeconds(cdp);
      samples[scenario].push({ latencyUs: (elapsedMs * 1000) / navsPerBatch, cpuUs: ((cpuAfter - cpuBefore) * 1e6) / navsPerBatch });
    }
  }
  assertClean(errors, router);
  await context.close();
  return { bootMs, samples };
}

async function loadHome(browser, url) {
  const opened = await openPage(browser);
  const scripts = new Set();
  opened.page.on("request", (request) => {
    if (request.resourceType() === "script") scripts.add(new URL(request.url()).pathname);
  });
  await opened.page.goto(`${url}/`);
  await opened.page.evaluate(() => window.__bench.page("home"));
  return { ...opened, scripts };
}

const navigateMany = (page, count) =>
  page.evaluate(async (n) => {
    await window.__bench.run("swap", n / 2);
    await window.__bench.run("param", n / 2);
    await window.__bench.run("home", 1);
  }, count);

async function measureMemory(browser, routerUrl, baselineUrl, router) {
  const baseline = await loadHome(browser, baselineUrl);
  const baselineHeap = await heapBytes(baseline.cdp);
  assertClean(baseline.errors, `${router} baseline`);
  await baseline.context.close();

  const { context, page, cdp, errors, scripts } = await loadHome(browser, routerUrl);
  const bootHeap = await heapBytes(cdp);
  const homeScripts = [...scripts];
  await navigateMany(page, retainedNavs);
  const warmHeap = await heapBytes(cdp);
  await navigateMany(page, retainedNavs);
  const afterHeap = await heapBytes(cdp);
  assertClean(errors, router);
  await context.close();
  return { baselineHeap, bootHeap, retained: afterHeap - warmHeap, homeScripts, baselineScripts: [...baseline.scripts] };
}

function summarize(router, perfRuns, memoryRuns, sizes) {
  const perf = { bootMs: median(perfRuns.map((r) => r.bootMs)) };
  for (const scenario of scenarios) {
    const pooled = perfRuns.flatMap((r) => r.samples[scenario]);
    perf[scenario] = { latencyUs: median(pooled.map((s) => s.latencyUs)), cpuUs: median(pooled.map((s) => s.cpuUs)) };
  }
  const pick = (f) => median(memoryRuns.map(f));
  const memory = {
    heap: pick((m) => m.bootHeap),
    cost: pick((m) => m.bootHeap - m.baselineHeap),
    retained: pick((m) => m.retained),
  };
  const { homeScripts, baselineScripts } = memoryRuns[0];
  const home = sumSizes(sizes.router, homeScripts);
  const total = sumSizes(sizes.router, sizes.router.keys());
  const bundle = {
    home,
    homeCost: diffSizes(home, sumSizes(sizes.baseline, baselineScripts)),
    total,
    totalCost: diffSizes(total, sumSizes(sizes.baseline, sizes.baseline.keys())),
  };
  return { router, bundle, perf, memory };
}

function report(summaries) {
  const reze = summaries.find((s) => s.router === "reze");
  const rel = (s, f) => (reze === undefined || s === reze ? "" : ` (${ratio(f(s), f(reze))})`);
  for (const metric of ["gzip", "brotli"]) {
    console.log(`\n## Bundle, ${metric} KiB (router cost = router app − same app without router)\n`);
    console.log("| router | JS to render `/` | router cost at `/` | all JS | router cost, all JS |");
    console.log("| --- | ---: | ---: | ---: | ---: |");
    for (const s of summaries) {
      const b = s.bundle;
      const homeCost = kib(b.homeCost[metric]) + rel(s, (x) => x.bundle.homeCost[metric]);
      const totalCost = kib(b.totalCost[metric]) + rel(s, (x) => x.bundle.totalCost[metric]);
      console.log(`| ${s.router} | ${kib(b.home[metric])} | ${homeCost} | ${kib(b.total[metric])} | ${totalCost} |`);
    }
  }
  console.log(
    `\n## Navigation, µs per navigation (median of ${runs}×${batches} batches of ${navsPerBatch}; CPU throttle ${cpuThrottle}×)\n`,
  );
  console.log(
    "Latency: click or history call until the new view is in the DOM. CPU: main-thread task time, including work after the commit.\n",
  );
  console.log(
    "| router | boot `/users/1` ms | param change latency | param CPU | route swap latency | route swap CPU | back/forward latency | back/forward CPU |",
  );
  console.log("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
  for (const s of summaries) {
    const cells = scenarios.flatMap((scenario) => [
      us(s.perf[scenario].latencyUs) + rel(s, (x) => x.perf[scenario].latencyUs),
      us(s.perf[scenario].cpuUs) + rel(s, (x) => x.perf[scenario].cpuUs),
    ]);
    console.log(`| ${s.router} | ${s.perf.bootMs.toFixed(1)} | ${cells.join(" | ")} |`);
  }
  console.log(`\n## Memory, JS heap KiB after forced GC (median of ${runs})\n`);
  console.log(`| router | heap at \`/\` | router cost | growth per ${retainedNavs} navs after warm-up |`);
  console.log("| --- | ---: | ---: | ---: |");
  for (const s of summaries) {
    const m = s.memory;
    console.log(`| ${s.router} | ${kib(m.heap)} | ${kib(m.cost)}${rel(s, (x) => x.memory.cost)} | ${signed(m.retained, kib)} |`);
  }
}

if (!args["skip-build"]) {
  buildPackages();
  await Promise.all(selected.map(build));
}

const servers = {};
const sizes = {};
for (const router of selected) {
  servers[router] = {};
  sizes[router] = {};
  for (const variant of variants) {
    servers[router][variant] = await serve(distDir(router, variant));
    sizes[router][variant] = assetSizes(distDir(router, variant));
  }
}

const browser = await chromium.launch();
const browserVersion = browser.version();
const perfRuns = Object.fromEntries(selected.map((r) => [r, []]));
const memoryRuns = Object.fromEntries(selected.map((r) => [r, []]));
try {
  for (let r = 0; r < runs; r++) {
    const order = selected.map((_, i) => selected[(i + r) % selected.length]);
    for (const router of order) {
      const routerUrl = origin(servers[router].router);
      perfRuns[router].push(await measurePerf(browser, routerUrl, router));
      memoryRuns[router].push(await measureMemory(browser, routerUrl, origin(servers[router].baseline), router));
      console.error(`run ${r + 1}/${runs}: ${router} done`);
    }
  }
} finally {
  await browser.close();
  for (const byVariant of Object.values(servers)) for (const server of Object.values(byVariant)) server.close();
}

const summaries = selected.map((router) => summarize(router, perfRuns[router], memoryRuns[router], sizes[router]));
report(summaries);

writeResults(here, {
  browser: browserVersion,
  options: { runs, batches, navsPerBatch, cpuThrottle, sections, retainedNavs },
  versions: Object.fromEntries(selected.map((router) => [router, dependenciesOf(appDir(router))])),
  summaries,
});
