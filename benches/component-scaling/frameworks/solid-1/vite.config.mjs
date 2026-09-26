import solid from "vite-plugin-solid";

export default {
  root: "src",
  build: { outDir: "../dist", emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: [solid()],
};
