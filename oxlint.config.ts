import { defineConfig } from "oxlint";

export default defineConfig({
  ignorePatterns: ["**/*.gen.d.ts", "target", "dist", "packages/compiler/index.js", "packages/compiler/index.d.ts"],
});
