import { execFile, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, extname, join } from "node:path";
import { parseArgs, promisify } from "node:util";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

import { chromium } from "playwright";

import { generate, routers, variants } from "./generate.mjs";

const run = promisify(execFile);
const here = import.meta.dirname;
const root = join(here, "..", "..");

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
const selected = args.only ? args.only.split(",") : routers;
const scenarios = ["param", "swap", "pop"];
const retainedNavs = 1000;

for (const name of selected)
  if (!routers.includes(name)) throw new Error(`unknown router "${name}", expected one of ${routers.join(", ")}`);

const appDir = (router) => join(here, "apps", router);
const distDir = (router, variant) => join(appDir(router), "dist", variant);

async function build(router) {
  for (const variant of variants) {
    const src = join(appDir(router), "src", variant);
    rmSync(src, { recursive: true, force: true });
    for (const [file, content] of Object.entries(generate(router, variant, sections))) {
      mkdirSync(dirname(join(src, file)), { recursive: true });
      writeFileSync(join(src, file), content);
    }
    try {
      await run("pnpm", ["exec", "vite", "build", "--logLevel", "error"], {
        cwd: appDir(router),
        env: { ...process.env, BENCH_VARIANT: variant },
        maxBuffer: 1 << 26,
      });
    } catch (error) {
      throw new Error(`${router} ${variant} build failed:\n${error.stdout}\n${error.stderr}`);
    }
  }
}

const brotli = (buf) => brotliCompressSync(buf, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length;

function assetSizes(dist) {
  const sizes = new Map();
  for (const file of readdirSync(join(dist, "assets"))) {
    if (!file.endsWith(".js")) continue;
    const content = readFileSync(join(dist, "assets", file));
    sizes.set(`/assets/${file}`, { raw: content.length, gzip: gzipSync(content, { level: 9 }).length, brotli: brotli(content) });
  }
  return sizes;
}

function sumSizes(sizes, paths) {
  const total = { raw: 0, gzip: 0, brotli: 0 };
  for (const path of paths) {
    const size = sizes.get(path);
    if (size === undefined) throw new Error(`loaded script ${path} is not a built asset`);
    total.raw += size.raw;
    total.gzip += size.gzip;
    total.brotli += size.brotli;
  }
  return total;
}

const diffSizes = (a, b) => ({ raw: a.raw - b.raw, gzip: a.gzip - b.gzip, brotli: a.brotli - b.brotli });

const mimeTypes = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css" };

function serve(dist) {
  const server = createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const file = join(dist, path);
    const isAsset = path !== "/" && file.startsWith(dist) && existsSync(file) && extname(file) !== "";
    const target = isAsset ? file : join(dist, "index.html");
    res.writeHead(200, {
      "content-type": mimeTypes[extname(target)] ?? "application/octet-stream",
      "cross-origin-opener-policy": "same-origin",
      "cross-origin-embedder-policy": "require-corp",
    });
    res.end(readFileSync(target));
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

const origin = (server) => `http://127.0.0.1:${server.address().port}`;

async function openPage(browser) {
  const context = await browser.newContext();
  await context.addInitScript({ path: join(here, "harness.js") });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error));
  const cdp = await context.newCDPSession(page);
  if (cpuThrottle > 1) await cdp.send("Emulation.setCPUThrottlingRate", { rate: cpuThrottle });
  return { context, page, cdp, errors };
}

async function heapBytes(cdp) {
  await cdp.send("HeapProfiler.collectGarbage");
  await cdp.send("HeapProfiler.collectGarbage");
  return (await cdp.send("Runtime.getHeapUsage")).usedSize;
}

async function taskSeconds(cdp) {
  const { metrics } = await cdp.send("Performance.getMetrics");
  return metrics.find((m) => m.name === "TaskDuration").value;
}

function assertClean(errors, router) {
  if (errors.length > 0) throw new Error(`${router} page errors:\n${errors.map(String).join("\n")}`);
}

async function measurePerf(browser, url, router) {
  const { context, page, cdp, errors } = await openPage(browser);
  await cdp.send("Performance.enable");
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

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
};

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

function versionsOf(router) {
  const pkg = JSON.parse(readFileSync(join(appDir(router), "package.json"), "utf8"));
  return pkg.dependencies;
}

const kb = (bytes) => (bytes / 1024).toFixed(2);
const signed = (value, format) => (value > 0 ? "+" : "") + format(value);
const us = (value) => value.toFixed(1);
const ratio = (value, reze) => (reze > 0 ? `${(value / reze).toFixed(2)}×` : "");

function report(summaries) {
  const reze = summaries.find((s) => s.router === "reze");
  const rel = (s, f) => (reze === undefined || s === reze ? "" : ` (${ratio(f(s), f(reze))})`);
  for (const metric of ["gzip", "brotli"]) {
    console.log(`\n## Bundle, ${metric} KiB (router cost = router app − same app without router)\n`);
    console.log("| router | JS to render `/` | router cost at `/` | all JS | router cost, all JS |");
    console.log("| --- | ---: | ---: | ---: | ---: |");
    for (const s of summaries) {
      const b = s.bundle;
      const homeCost = kb(b.homeCost[metric]) + rel(s, (x) => x.bundle.homeCost[metric]);
      const totalCost = kb(b.totalCost[metric]) + rel(s, (x) => x.bundle.totalCost[metric]);
      console.log(`| ${s.router} | ${kb(b.home[metric])} | ${homeCost} | ${kb(b.total[metric])} | ${totalCost} |`);
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
    console.log(`| ${s.router} | ${kb(m.heap)} | ${kb(m.cost)}${rel(s, (x) => x.memory.cost)} | ${signed(m.retained, kb)} |`);
  }
}

if (!args["skip-build"]) {
  execFileSync("pnpm", ["build"], { cwd: root, stdio: "inherit" });
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

const out = join(here, "results", "latest.json");
mkdirSync(dirname(out), { recursive: true });
writeFileSync(
  out,
  `${JSON.stringify(
    {
      schema: 1,
      recordedAt: new Date().toISOString(),
      browser: browserVersion,
      options: { runs, batches, navsPerBatch, cpuThrottle, sections, retainedNavs },
      versions: Object.fromEntries(selected.map((router) => [router, versionsOf(router)])),
      summaries,
    },
    null,
    2,
  )}\n`,
);
console.log(`\nwrote ${out}`);
