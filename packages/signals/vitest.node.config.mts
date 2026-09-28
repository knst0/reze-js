import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    name: "@rezejs/signals (node)",
    include: ["tests/**/*.node.spec.ts"],
  },
});
