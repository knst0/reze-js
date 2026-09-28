import { configDefaults, defineConfig } from "vitest/config";

import { browserConfig, sourceAliases } from "../../vitest.shared";
import reze from "../vite-plugin/src/index";

export default defineConfig({
  plugins: [reze({ links: "@rezejs/router" })],
  test: {
    name: "@rezejs/router",
    exclude: [...configDefaults.exclude, "tests/**/*.node.spec.ts"],
    environment: "happy-dom",
    environmentOptions: { happyDOM: { url: "http://localhost/" } },
    alias: sourceAliases,
    browser: browserConfig(),
  },
});
