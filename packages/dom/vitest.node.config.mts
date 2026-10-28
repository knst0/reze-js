import { defineConfig } from "vite-plus";

export default defineConfig({
  test: {
    name: "@rezejs/dom/node",
    include: ["tests/**/*.node.spec.ts"],
  },
});
