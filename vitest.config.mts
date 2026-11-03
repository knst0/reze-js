import { defineConfig } from "vite-plus";

export default defineConfig({
  test: {
    projects: ["tests/compiler/vitest.config.mts"],
  },
});
