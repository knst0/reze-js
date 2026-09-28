import reze from "@rezejs/vite-plugin";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [reze({ fileRoutes: true })],
  build: { modulePreload: { polyfill: false } },
});
