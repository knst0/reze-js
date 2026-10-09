import { octane } from "@octanejs/vite-plugin";
import { lazyPlugins } from "vite-plus";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [octane({ strong: true })]),
};
