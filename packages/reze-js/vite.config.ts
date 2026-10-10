import { defineConfig } from "vite-plus";

export default defineConfig({
  pack: {
    deps: {
      // tsdown <0.23 compatibility: resolve external dependency subpaths.
      // Remove to preserve subpath imports as written (the new default).
      // https://tsdown.dev/options/dependencies#deps-resolvedepsubpath
      resolveDepSubpath: true,
    },
    entry: [
      "src/index.ts",
      "src/jsx-runtime.ts",
      "src/jsx-dev-runtime.ts",
      "src/internal/reactivity.ts",
      "src/internal/runtime.ts",
      "src/internal/html.ts",
      "src/internal/client.ts",
      "src/server/node.ts",
      "src/server/bun.ts",
    ],
    platform: "neutral",
    unbundle: true,
    dts: { tsconfig: "./tsconfig.build.json" },
  },
});
