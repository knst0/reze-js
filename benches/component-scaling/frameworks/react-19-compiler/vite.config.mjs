import babel from "@rolldown/plugin-babel";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";
import { lazyPlugins } from "vite-plus";

export default {
  root: "src",
  build: { outDir: "../dist", emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: lazyPlugins(() => [react(), babel({ presets: [reactCompilerPreset()] })]),
};
