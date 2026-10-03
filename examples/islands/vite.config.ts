import reze from "@rezejs/vite-plugin";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [reze({ prerender: true })],
  build: { modulePreload: { polyfill: false } },
});
