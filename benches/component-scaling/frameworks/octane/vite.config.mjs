import { octane } from "@octanejs/vite-plugin";

export default {
  root: "src",
  build: { outDir: "../dist", emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: [octane()],
};
