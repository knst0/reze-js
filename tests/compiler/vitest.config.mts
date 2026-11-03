import { defineConfig } from "vite-plus";

import { sourceAliases } from "../../vitest.shared";

export default defineConfig({
  resolve: { conditions: ["reze"] },
  ssr: { resolve: { conditions: ["reze"] } },
  test: {
    name: "compiler",
    include: ["fixtures.test.ts"],
    root: import.meta.dirname,
    alias: sourceAliases,
  },
});
