import { defineConfig } from "vitest/config";

// Shared by all workers in this run; tags history.jsonl lines for merge.mjs.
process.env.REZE_BENCH_RUN ??= new Date().toISOString();

export default defineConfig({
  test: {
    environment: "node",
  },
});
