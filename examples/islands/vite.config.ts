import reze from "@rezejs/vite-plugin";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [reze({ ssg: { entry: "src/app.tsx" } })],
  build: { modulePreload: { polyfill: false } },
});
