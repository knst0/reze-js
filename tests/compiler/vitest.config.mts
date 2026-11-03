import { defineConfig } from "vite-plus";

import { sourceAliases } from "../../vitest.shared";

export default defineConfig({
  test: {
    name: "compiler",
    include: ["fixtures.test.ts"],
    root: import.meta.dirname,
    alias: sourceAliases,
  },
});
