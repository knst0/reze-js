import { join } from "node:path";

import { buildSync } from "esbuild";
import { defineConfig } from "vitest/config";

// Shared by all workers in this run; tags history.jsonl lines for merge.mjs.
process.env.REZE_BENCH_RUN ??= new Date().toISOString();

const productionSignalsBundle = join(import.meta.dirname, "dist", "signals.mjs");
buildSync({
  entryPoints: [join(import.meta.dirname, "..", "..", "packages", "signals", "src", "index.ts")],
  outfile: productionSignalsBundle,
  bundle: true,
  format: "esm",
  platform: "neutral",
  define: { "process.env.NODE_ENV": '"production"' },
  logLevel: "warning",
});

export default defineConfig({
  resolve: {
    alias: { "@rezejs/signals": productionSignalsBundle },
  },
  test: {
    environment: "node",
  },
});
