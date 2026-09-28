import vue from "@vitejs/plugin-vue";

const variant = process.env.BENCH_VARIANT ?? "router";

export default {
  root: `src/${variant}`,
  build: { outDir: `../../dist/${variant}`, emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: [vue()],
  define: { __VUE_OPTIONS_API__: false },
};
