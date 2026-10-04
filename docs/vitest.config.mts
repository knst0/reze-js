import { defineConfig } from "vitest/config";

export default defineConfig({
  root: import.meta.dirname,
  test: {
    name: "@rezejs/docs",
    include: ["plugins/**/*.spec.ts"],
  },
});
