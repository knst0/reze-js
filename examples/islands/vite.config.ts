import reze from "@rezejs/vite-plugin";
import { defineConfig, lazyPlugins } from "vite-plus";

export default defineConfig({
  plugins: lazyPlugins(() => [reze({ ssg: { entry: "src/app.tsx" } })]),
  build: { modulePreload: { polyfill: false } },
});
