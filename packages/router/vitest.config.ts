import reze from "@rezejs/vite-plugin";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [reze({ moduleName: "@rezejs/dom" })],
  test: {
    environment: "happy-dom",
  },
});
