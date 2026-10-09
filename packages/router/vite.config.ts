import { defineConfig } from "vite-plus";

import { rezeCompiler } from "./reze-compiler-plugin";

export default defineConfig({
  pack: {
    entry: ["src/index.ts", "src/fs/index.ts", "src/internal/server.ts", "src/internal/swap.ts"],
    platform: "neutral",
    unbundle: true,
    deps: {
      // tsdown <0.23 compatibility: resolve external dependency subpaths.
      // Remove to preserve subpath imports as written (the new default).
      // https://tsdown.dev/options/dependencies#deps-resolvedepsubpath
      resolveDepSubpath: true,
      neverBundle: [/^node:/],
    },
    plugins: [rezeCompiler],
    dts: { tsconfig: "./tsconfig.build.json" },
  },
});
