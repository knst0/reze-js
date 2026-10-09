import reze from "@rezejs/vite-plugin";
import { lazyPlugins } from "vite-plus";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [reze()]),
};
