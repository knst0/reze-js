import { octane } from "@octanejs/vite-plugin";
import { lazyPlugins } from "vite-plus";

export default {
  root: "src",
  build: { outDir: "../dist", emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [octane()]),
};
