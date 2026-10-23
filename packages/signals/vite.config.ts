import { defineConfig } from "vite-plus";

const shared = {
  deps: {
    // tsdown <0.23 compatibility: resolve external dependency subpaths.
    // Remove to preserve subpath imports as written (the new default).
    // https://tsdown.dev/options/dependencies#deps-resolvedepsubpath
    resolveDepSubpath: true,
  },
  entry: [
    "src/index.ts",
    "src/profile.ts",
    "src/render.ts",
    "src/internal/continuation.ts",
    "src/internal/scope.ts",
    "src/internal/resource.ts",
  ],
  platform: "neutral" as const,
  unbundle: true,
};

export default defineConfig({
  pack: [
    {
      ...shared,
      outDir: "dist",
      dts: { tsconfig: "./tsconfig.build.json" },
      define: { __REZE_HTML__: "false" },
    },
    {
      ...shared,
      outDir: "dist/html",
      dts: false,
      define: { __REZE_HTML__: "true" },
    },
  ],
});
