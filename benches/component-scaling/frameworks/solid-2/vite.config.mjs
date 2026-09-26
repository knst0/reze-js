import solid from "@solidjs/vite-plugin";

export default {
  root: "src",
  build: { outDir: "../dist", emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: [solid()],
};
