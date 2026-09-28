import { octane } from "@octanejs/vite-plugin";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: [octane({ strong: true })],
};
