import { join } from "node:path";

import { rolldown } from "rolldown";
import { defineConfig, type Plugin } from "vitest/config";

const devBuild = join(import.meta.dirname, "dist", "index.js");
const productionBundle = join(import.meta.dirname, "node_modules", ".bench", "signals.mjs");

async function buildProductionBundle(): Promise<void> {
  const build = await rolldown({
    input: join(import.meta.dirname, "src", "index.ts"),
    platform: "neutral",
    transform: { define: { "process.env.NODE_ENV": '"production"' } },
  });
  await build.write({ file: productionBundle, format: "esm", minify: true });
  await build.close();
}

let productionBuild: Promise<void> | undefined;

const resolveSignals: Plugin = {
  name: "rezejs:resolve-signals",
  enforce: "pre",
  async resolveId(id, importer) {
    if (id !== "@rezejs/signals") return;
    if (!importer?.endsWith(".bench.ts")) return devBuild;
    await (productionBuild ??= buildProductionBundle());
    return productionBundle;
  },
};

export default defineConfig({
  plugins: [resolveSignals],
  test: {
    benchmark: { include: ["benches/**/*.bench.ts"] },
  },
});
