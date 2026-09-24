import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join } from "node:path";

import { chromium } from "playwright-core";

const here = new URL(".", import.meta.url).pathname;
const dist = join(here, "dist");
const resultsDir = join(here, "results");
const iterations = 12;
const scenarios = [
  { name: "create 1k", run: "create1k" },
  { name: "update every 10th", setup: "create1k", run: "update10th" },
  { name: "swap rows", setup: "create1k", run: "swap" },
  { name: "select row", setup: "create1k", run: "select" },
  { name: "remove row", setup: "create1k", run: "remove" },
  { name: "append 1k", setup: "create1k", run: "append" },
  { name: "create 10k", run: "create10k" },
  { name: "clear 10k", setup: "create10k", run: "clear" },
];
const contentTypes = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css" };

function serve() {
  const server = createServer((request, response) => {
    const path = join(dist, request.url === "/" ? "index.html" : request.url);
    if (!path.startsWith(dist) || !existsSync(path)) {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, {
      "content-type": contentTypes[extname(path)] ?? "application/octet-stream",
    });
    response.end(readFileSync(path));
  });
  return new Promise((resolve) => server.listen(0, () => resolve(server)));
}

async function measureInPage({ scenarios, iterations }) {
  const ops = window.ops;
  const settle = () => new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve)));
  const timeOp = async (name) => {
    const start = performance.now();
    ops[name]();
    ops.flush();
    void document.body.offsetHeight;
    const elapsedMs = performance.now() - start;
    await settle();
    return elapsedMs;
  };
  await timeOp("create1k");
  const rowCount = document.querySelectorAll("tr").length;
  if (rowCount !== 1000) throw new Error(`expected 1000 rows after create1k, got ${rowCount}`);
  const medians = {};
  for (const scenario of scenarios) {
    const samples = [];
    for (let i = 0; i < iterations; i++) {
      await timeOp("clear");
      globalThis.gc?.();
      if (scenario.setup) await timeOp(scenario.setup);
      samples.push(await timeOp(scenario.run));
    }
    samples.sort((a, b) => a - b);
    medians[scenario.name] = samples[Math.floor(samples.length / 2)];
  }
  return medians;
}

const server = await serve();
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM_PATH ?? "/opt/pw-browsers/chromium",
  args: ["--js-flags=--expose-gc"],
});
let medians;
try {
  const page = await browser.newPage();
  await page.goto(`http://localhost:${server.address().port}/`);
  await page.waitForFunction(() => "ops" in window);
  medians = await page.evaluate(measureInPage, { scenarios, iterations });
} finally {
  await browser.close();
  server.close();
}

let previous = {};
try {
  previous = JSON.parse(readFileSync(join(resultsDir, "latest.json"), "utf8")).results;
} catch {
  previous = {};
}

const width = Math.max(...scenarios.map((s) => s.name.length));
const results = {};
for (const { name } of scenarios) {
  const medianMs = Number(medians[name].toFixed(2));
  results[name] = { medianMs };
  const was = previous[name]?.medianMs;
  const delta =
    was === undefined
      ? "  (first run)"
      : `  (was ${was} ms, ${medianMs >= was ? "+" : ""}${((medianMs / was - 1) * 100).toFixed(1)}%)`;
  console.log(`${name.padEnd(width)}  ${medianMs.toFixed(2).padStart(8)} ms${delta}`);
}

mkdirSync(resultsDir, { recursive: true });
const path = join(resultsDir, "latest.json");
writeFileSync(
  path,
  `${JSON.stringify({ schema: 1, recordedAt: new Date().toISOString(), results }, null, 2)}\n`,
);
console.log(`wrote ${path}`);
