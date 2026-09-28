import vue from "@vitejs/plugin-vue";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: [vue()],
};
