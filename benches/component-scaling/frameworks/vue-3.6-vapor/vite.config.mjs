import vue from "@vitejs/plugin-vue";
import { lazyPlugins } from "vite-plus";

export default {
  root: "src",
  build: { outDir: "../dist", emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [vue()]),
  define: { __VUE_OPTIONS_API__: false },
};
