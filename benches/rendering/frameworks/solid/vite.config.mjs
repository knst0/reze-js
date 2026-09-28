import solid from "vite-plugin-solid";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: [solid()],
};
