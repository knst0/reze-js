import { defineConfig } from "oxlint";

export default defineConfig({
  ignorePatterns: ["target", "dist", "packages/compiler/index.js", "packages/compiler/index.d.ts"],
});
