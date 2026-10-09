import solid from "vite-plugin-solid";
import { lazyPlugins } from "vite-plus";

const variant = process.env.BENCH_VARIANT ?? "router";

export default {
  root: `src/${variant}`,
  build: { outDir: `../../dist/${variant}`, emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [solid()]),
};
