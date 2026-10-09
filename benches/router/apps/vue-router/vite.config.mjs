import vue from "@vitejs/plugin-vue";
import { lazyPlugins } from "vite-plus";

const variant = process.env.BENCH_VARIANT ?? "router";

export default {
  root: `src/${variant}`,
  build: { outDir: `../../dist/${variant}`, emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [vue()]),
  define: { __VUE_OPTIONS_API__: false },
};
