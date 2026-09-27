import { defineConfig } from "oxfmt";

export default defineConfig({
  ignorePatterns: ["target", "dist", "packages/compiler/skills/reze-compiler-diagnostics/SKILL.md", "packages/compiler/index.js", "packages/compiler/index.d.ts"],
  sortImports: true,
  sortPackageJson: {
    sortScripts: true,
  },
});
