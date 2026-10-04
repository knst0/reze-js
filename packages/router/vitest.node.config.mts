import { join } from "node:path";
import { defineConfig } from "vitest/config";

import { nodeSpecs, sourceAliases } from "../../vitest.shared";

const root = join(import.meta.dirname, "..", "..");
export default defineConfig({
  define: { __REZE_HTML__: "false", __REZE_HYDRATE__: "false" },
  test: {
    name: "@rezejs/router (node)",
    include: [nodeSpecs],
    alias: [
      { find: "@rezejs/dom/internal/html", replacement: join(root, "packages/dom/src/internal/html.ts") },
      ...sourceAliases,
    ],
  },
});
