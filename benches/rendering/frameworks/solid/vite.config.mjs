import solid from "vite-plugin-solid";
import { lazyPlugins } from "vite-plus";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [solid()]),
};
