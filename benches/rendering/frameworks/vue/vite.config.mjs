import vue from "@vitejs/plugin-vue";
import { lazyPlugins } from "vite-plus";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [vue()]),
};
