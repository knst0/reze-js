import { join } from "node:path";

import { rolldown } from "rolldown";
import { configDefaults, defineConfig, type Plugin } from "vitest/config";

import { browserConfig, nodeSpecs } from "../../vitest.shared";

const sourceEntry = join(import.meta.dirname, "src", "index.ts");
const productionBundle = join(import.meta.dirname, "node_modules", ".bench", "signals.mjs");

async function buildProductionBundle(): Promise<void> {
  const build = await rolldown({
    input: sourceEntry,
    platform: "neutral",
    transform: {
      define: {
        "process.env.NODE_ENV": '"production"',
        __REZE_HTML__: "false",
        __REZE_HYDRATE__: "false",
      },
    },
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
    if (!importer?.endsWith(".bench.ts")) return sourceEntry;
    await (productionBuild ??= buildProductionBundle());
    return productionBundle;
  },
};

export default defineConfig({
  plugins: [resolveSignals],
  define: {
    __REZE_HTML__: "false",
    __REZE_HYDRATE__: "false",
  },
  test: {
    name: "@rezejs/signals",
    exclude: [...configDefaults.exclude, nodeSpecs],
    benchmark: { include: ["benches/**/*.bench.ts"] },
    browser: browserConfig(),
  },
});
