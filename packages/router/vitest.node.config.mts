import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    name: "@rezejs/router (node)",
    include: ["tests/**/*.node.spec.ts"],
  },
});
