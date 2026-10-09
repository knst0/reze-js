import { existsSync } from "node:fs";
import { join } from "node:path";

import type { BenchFn, TestContext } from "vite-plus/test";

const resultsDir = join(import.meta.dirname, "..", "results");
const isUpdate = process.env.REZE_BENCH_UPDATE === "1";

function toFileName(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

/**
 * Runs `fn` and compares it with the committed baseline in `benches/results/<test>/<name>.json`.
 * The baseline is written only when missing or when `REZE_BENCH_UPDATE=1`.
 */
export async function measure({ bench, task }: TestContext, name: string, fn: BenchFn): Promise<void> {
  const baseline = join(resultsDir, toFileName(task.name), `${toFileName(name)}.json`);
  const hasBaseline = existsSync(baseline);
  const current = hasBaseline && !isUpdate ? bench(name, fn) : bench(name, { writeResult: baseline }, fn);
  if (hasBaseline) await bench.compare(current, bench.from(`${name} (baseline)`, baseline));
  else await current.run();
}
