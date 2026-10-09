import { defineConfig } from "vite-plus";

export default defineConfig({
  test: {
    projects: ["packages/*/vitest.config.mts", "packages/*/vitest.node.config.mts", "docs/vitest.config.mts"],
  },
});
