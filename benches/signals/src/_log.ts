import { appendFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import type { BenchResult } from "vitest";

// Appends one JSON line per benchmark; scripts/merge.mjs compacts them into
// results/latest.json. Workers share the run id via process env (see vitest.config.ts).
const run = process.env.REZE_BENCH_RUN ?? "adhoc";
const file = join(import.meta.dirname, "..", "results", "history.jsonl");
mkdirSync(dirname(file), { recursive: true });

export function logResult(group: string, result: BenchResult): void {
  appendFileSync(
    file,
    `${JSON.stringify({
      run,
      group,
      name: result.name,
      hz: result.throughput.mean,
      meanMs: result.latency.mean,
      p50Ms: result.latency.p50,
    })}\n`,
  );
}
