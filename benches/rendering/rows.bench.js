import { afterAll, beforeAll, expect, test } from "vitest";
import { cdp, server } from "vitest/browser";

import { mount } from "./app.jsx";

const ROWS = 1000;
const RUN_OPTIONS = { time: 0, iterations: 1000 };
const MEMORY_RUNS = 5;
const WARMUP_CYCLES = 10;
const RETAINED_CYCLES = 100;

const resultsDir = `results/${server.browser}`;
const isUpdate = import.meta.env.REZE_BENCH_UPDATE === "1";

let app;

beforeAll(() => {
  const container = document.createElement("div");
  document.body.append(container);
  app = mount(container);
});

afterAll(() => app.dispose());

function toFileName(name) {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

async function readBaseline(path) {
  try {
    return await server.commands.readFile(path);
  } catch {
    return undefined;
  }
}

/**
 * Runs `fn` and compares it with the committed baseline in `results/<browser>/<name>.json`.
 * The baseline is written only when missing or when `REZE_BENCH_UPDATE=1`.
 */
async function measure({ bench }, name, hooks, fn) {
  const baseline = `${resultsDir}/${toFileName(name)}.json`;
  const hasBaseline = (await readBaseline(baseline)) !== undefined;
  const current = hasBaseline && !isUpdate ? bench(name, hooks, fn) : bench(name, { ...hooks, writeResult: baseline }, fn);
  if (hasBaseline) await bench.compare(current, bench.from(`${name} (baseline)`, baseline), RUN_OPTIONS);
  else await current.run(RUN_OPTIONS);
}

test("rows", async (context) => {
  const createRows = () => app.create(ROWS);
  await measure(context, `create ${ROWS} rows`, { beforeEach: () => app.clear() }, createRows);
  await measure(context, `update every 10th of ${ROWS} rows`, { beforeEach: createRows }, () => app.updateEveryTenth());
  await measure(context, `swap 2 of ${ROWS} rows`, { beforeAll: createRows }, () => app.swap());
  await measure(context, `clear ${ROWS} rows`, { beforeEach: createRows }, () => app.clear());
  app.clear();
});

async function heapBytes(session) {
  await session.send("HeapProfiler.collectGarbage");
  await session.send("HeapProfiler.collectGarbage");
  return (await session.send("Runtime.getHeapUsage")).usedSize;
}

function cycle() {
  app.create(ROWS);
  app.updateEveryTenth();
  app.swap();
  app.clear();
}

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 === 1 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
};

const kib = (bytes) => `${(bytes / 1024).toFixed(1)} KiB`;

const signedKib = (bytes) => `${bytes > 0 ? "+" : ""}${kib(bytes)}`;

function formatMemory(name, current, previous) {
  if (previous === undefined) return `${name}: ${kib(current)}`;
  return `${name}: ${kib(current)} (baseline ${kib(previous)}, ${signedKib(current - previous)})`;
}

test.runIf(server.browser === "chromium")("memory", async () => {
  const session = cdp();
  const rowHeaps = [];
  const retainedHeaps = [];
  for (let run = 0; run < MEMORY_RUNS; run++) {
    app.clear();
    const emptyHeap = await heapBytes(session);
    app.create(ROWS);
    expect(app.rowCount()).toBe(ROWS);
    rowHeaps.push((await heapBytes(session)) - emptyHeap);

    app.clear();
    for (let i = 0; i < WARMUP_CYCLES; i++) cycle();
    const warmHeap = await heapBytes(session);
    for (let i = 0; i < RETAINED_CYCLES; i++) cycle();
    retainedHeaps.push((await heapBytes(session)) - warmHeap);
  }

  const results = {
    [`heap of ${ROWS} rows`]: median(rowHeaps),
    [`retained after ${RETAINED_CYCLES} create/update/swap/clear cycles`]: median(retainedHeaps),
  };
  const baselinePath = `${resultsDir}/memory.json`;
  const saved = await readBaseline(baselinePath);
  const baseline = saved === undefined ? {} : JSON.parse(saved);
  console.log(
    [`memory, JS heap after forced GC (median of ${MEMORY_RUNS})`]
      .concat(Object.entries(results).map(([name, bytes]) => `  ${formatMemory(name, bytes, baseline[name])}`))
      .join("\n"),
  );
  if (saved === undefined || isUpdate) await server.commands.writeFile(baselinePath, `${JSON.stringify(results, null, 2)}\n`);
});
