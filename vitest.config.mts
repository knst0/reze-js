import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    projects: ["packages/*/vitest.config.mts", "packages/*/vitest.node.config.mts", "docs/vitest.config.mts"],
  },
});
