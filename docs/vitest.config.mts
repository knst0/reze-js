import { defineConfig } from "vite-plus";

export default defineConfig({
  root: import.meta.dirname,
  test: {
    name: "@rezejs/docs",
    include: ["plugins/**/*.spec.ts"],
  },
});
