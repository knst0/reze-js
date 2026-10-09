import react from "@vitejs/plugin-react";
import { lazyPlugins } from "vite-plus";

export default {
  root: "src",
  build: { outDir: "../dist", emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [react()]),
};
