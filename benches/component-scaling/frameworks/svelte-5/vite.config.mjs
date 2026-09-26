import { svelte } from "@sveltejs/vite-plugin-svelte";

export default {
  root: "src",
  build: { outDir: "../dist", emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: [svelte()],
};
