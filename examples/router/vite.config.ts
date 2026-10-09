import reze from "@rezejs/vite-plugin";
import { defineConfig, lazyPlugins } from "vite-plus";

export default defineConfig({
  plugins: lazyPlugins(() => [reze({ fileRoutes: true })]),
  build: { modulePreload: { polyfill: false } },
});
