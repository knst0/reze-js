import reze from "@rezejs/vite-plugin";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: [reze()],
};
