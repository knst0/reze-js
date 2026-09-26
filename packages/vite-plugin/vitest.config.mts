import { join } from "node:path";

import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    alias: {
      "@rezejs/vite-plugin": join(import.meta.dirname, "dist", "index.js"),
    },
  },
});
