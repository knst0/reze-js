import { defineConfig } from "vite-plus";

export default defineConfig({
  test: {
    name: "@rezejs/query (node)",
    include: ["tests/**/*.node.spec.ts"],
  },
});
