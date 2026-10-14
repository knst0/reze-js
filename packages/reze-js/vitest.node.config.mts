import { defineConfig } from "vite-plus";

export default defineConfig({
  test: {
    name: "reze-js (node)",
    include: ["tests/**/*.node.spec.ts"],
  },
});
