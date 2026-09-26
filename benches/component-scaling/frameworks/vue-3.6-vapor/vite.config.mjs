import vue from "@vitejs/plugin-vue";

export default {
  root: "src",
  build: { outDir: "../dist", emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: [vue()],
  define: { __VUE_OPTIONS_API__: false },
};
