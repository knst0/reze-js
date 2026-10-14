import { defineConfig } from "vite-plus";

export default defineConfig({
  test: {
    name: "@rezejs/compiler (node)",
    include: ["tests/**/*.node.spec.ts"],
  },
});
