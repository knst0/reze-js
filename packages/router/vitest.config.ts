import { fileRoutes } from "@rezejs/router/vite";
import reze from "@rezejs/vite-plugin";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [reze({ moduleName: "@rezejs/dom" }), fileRoutes({ dir: "tests/fixtures/routes" })],
  test: {
    environment: "happy-dom",
  },
});
