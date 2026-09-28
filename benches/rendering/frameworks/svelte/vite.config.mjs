import { svelte } from "@sveltejs/vite-plugin-svelte";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: [svelte()],
};
