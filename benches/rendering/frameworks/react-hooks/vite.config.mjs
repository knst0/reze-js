import react from "@vitejs/plugin-react";
import { lazyPlugins } from "vite-plus";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [react()]),
};
