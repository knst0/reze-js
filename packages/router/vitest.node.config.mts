import { join } from "node:path";

import { defineConfig } from "vite-plus";

import { nodeSpecs, sourceAliases } from "../../vitest.shared";
import { rezeCompiler } from "./reze-compiler-plugin";

const root = join(import.meta.dirname, "..", "..");
export default defineConfig({
  plugins: [{ ...rezeCompiler, enforce: "pre" }],
  define: { __REZE_HTML__: "false" },
  test: {
    name: "@rezejs/router (node)",
    include: [nodeSpecs],
    alias: [{ find: "@rezejs/dom/internal/html", replacement: join(root, "packages/dom/src/internal/html.ts") }, ...sourceAliases],
  },
});
