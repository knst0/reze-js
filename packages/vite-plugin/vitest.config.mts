import { join } from "node:path";

import { defineConfig } from "vitest/config";

export default defineConfig({
  define: { __REZE_HTML__: "false", __REZE_HYDRATE__: "false" },
  test: {
    name: "@rezejs/vite-plugin",
    alias: {
      "@rezejs/router/fs": join(import.meta.dirname, "..", "router", "src", "fs", "index.ts"),
      "@rezejs/signals/profile": join(import.meta.dirname, "..", "signals", "src", "profile.ts"),
      "@rezejs/signals": join(import.meta.dirname, "..", "signals", "src", "index.ts"),
    },
  },
});
