import { defineConfig } from "oxlint";

export default defineConfig({
  ignorePatterns: [
    "**/*.gen.d.ts",
    "benches/rendering/frameworks/*/upstream",
    "target",
    "dist",
    "packages/compiler/index.js",
    "packages/compiler/index.d.ts",
  ],
});
