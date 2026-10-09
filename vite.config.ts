import { defineConfig } from "vite-plus";

const ignorePatterns = [
  "**/*.gen.d.ts",
  "benches/rendering/frameworks/*/upstream",
  "target",
  "dist",
  "packages/compiler/index.js",
  "packages/compiler/index.d.ts",
];

export default defineConfig({
  fmt: {
    ignorePatterns: [...ignorePatterns, "packages/compiler/skills/reze-compiler-diagnostics/SKILL.md"],
    sortImports: true,
    sortPackageJson: {
      sortScripts: true,
    },
  },
  lint: {
    ignorePatterns,
    jsPlugins: [{ name: "vite-plus", specifier: "vite-plus/oxlint-plugin" }],
    rules: { "vite-plus/prefer-vite-plus-imports": "error" },
    options: { typeAware: true, typeCheck: true },
  },
});
