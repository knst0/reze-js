import { svelte } from "@sveltejs/vite-plugin-svelte";
import { lazyPlugins } from "vite-plus";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [svelte()]),
};
