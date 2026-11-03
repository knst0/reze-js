import { defineConfig } from "vite-plus";

const fixtures = "tests/compiler/*/**";

const ignorePatterns = [
  "**/*.gen.d.ts",
  "**/benches/size/**",
  "target",
  "dist",
  "packages/compiler/index.js",
  "packages/compiler/index.d.ts",
  "docs",
];

export default defineConfig({
  fmt: {
    ignorePatterns: [...ignorePatterns, fixtures, "packages/compiler/skills/reze-compiler-diagnostics/SKILL.md"],
    sortImports: true,
    sortPackageJson: {
      sortScripts: true,
    },
  },
  lint: {
    ignorePatterns: [...ignorePatterns, fixtures],
    jsPlugins: [{ name: "vite-plus", specifier: "vite-plus/oxlint-plugin" }],
    rules: { "vite-plus/prefer-vite-plus-imports": "error" },
    options: { typeAware: true, typeCheck: true },
  },
});
