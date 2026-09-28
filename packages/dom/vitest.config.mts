import { defineConfig } from "vitest/config";

import { browserConfig, sourceAliases } from "../../vitest.shared";
import reze from "../vite-plugin/src/index";

export default defineConfig({
  plugins: [reze()],
  test: {
    environment: "happy-dom",
    alias: sourceAliases,
    browser: browserConfig(),
  },
});
