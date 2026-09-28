import { join } from "node:path";

import { build } from "vite";
import { defineConfig, type Plugin } from "vitest/config";

import reze from "../../packages/vite-plugin/src/index";
import { browserConfig, sourceAliases } from "../../vitest.shared";

const here = import.meta.dirname;
const appSource = join(here, "app.jsx");
const productionBundle = join(here, "node_modules", ".bench", "app.js");

async function buildProductionBundle(): Promise<void> {
  await build({
    configFile: false,
    logLevel: "warn",
    mode: "production",
    define: { "process.env.NODE_ENV": '"production"' },
    resolve: { alias: sourceAliases },
    plugins: [reze()],
    build: {
      outDir: join(productionBundle, ".."),
      emptyOutDir: true,
      minify: true,
      lib: { entry: appSource, formats: ["es"], fileName: () => "app.js" },
    },
  });
}

let productionBuild: Promise<void> | undefined;

const resolveProductionApp: Plugin = {
  name: "rezejs:resolve-production-app",
  enforce: "pre",
  async resolveId(id, importer) {
    if (id !== "./app.jsx" || !importer?.endsWith(".bench.js")) return;
    await (productionBuild ??= buildProductionBundle());
    return productionBundle;
  },
};

export default defineConfig({
  plugins: [resolveProductionApp],
  server: {
    headers: { "cross-origin-opener-policy": "same-origin", "cross-origin-embedder-policy": "require-corp" },
    watch: { ignored: [productionBundle] },
  },
  test: {
    benchmark: { include: ["*.bench.js"] },
    env: { REZE_BENCH_UPDATE: process.env.REZE_BENCH_UPDATE ?? "" },
    fileParallelism: false,
    testTimeout: 600_000,
    reporters: ["verbose"],
    browser: browserConfig(process.env.VITEST_ENV || "chromium"),
  },
});
