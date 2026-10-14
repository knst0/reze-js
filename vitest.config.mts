import { defineConfig } from "vite-plus";

export default defineConfig({
  test: {
    projects: ["packages/*/vitest.config.mts", "packages/*/vitest.node.config.mts", "docs/vitest.config.mts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "json-summary", "json"],
      reportsDirectory: "coverage",
      include: ["packages/*/src/**/*.{ts,tsx}"],
      exclude: [
        "**/*.d.ts",
        "packages/*/src/**/*.gen.*",
        "packages/reze-js/src/index.ts",
        "packages/reze-js/src/jsx-runtime.ts",
        "packages/reze-js/src/jsx-dev-runtime.ts",
        "packages/reze-js/src/internal/client.ts",
        "packages/reze-js/src/internal/html.ts",
        "packages/reze-js/src/internal/reactivity.ts",
        "packages/reze-js/src/internal/runtime.ts",
        "packages/dom/src/jsx.ts",
        "packages/dom/src/walk.ts",
        "packages/dom/src/html/module-state.ts",
        "packages/dom/src/html/session.ts",
        "packages/dom/src/html/site.ts",
        "packages/reze-js/src/server/bun.ts",
        "packages/router/src/define.ts",
        "packages/router/src/internal/swap.ts",
        "packages/signals/src/internal/resource.ts",
        "packages/vite-plugin/src/manifest-cli.ts",
      ],
      thresholds: {
        lines: 62,
        statements: 61,
        functions: 65,
        branches: 54,
        "packages/signals/src/**": { lines: 74, statements: 73, functions: 76, branches: 62 },
        "packages/dom/src/**": { lines: 30, statements: 30, functions: 32, branches: 23 },
      },
    },
  },
});
