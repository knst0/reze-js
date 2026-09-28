import babel from "@rolldown/plugin-babel";
import react, { reactCompilerPreset } from "@vitejs/plugin-react";

export default {
  build: { modulePreload: { polyfill: false } },
  plugins: [react(), babel({ presets: [reactCompilerPreset()] })],
};
