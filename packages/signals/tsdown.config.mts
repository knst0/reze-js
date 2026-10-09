import { defineConfig } from "tsdown";

const entry = [
  "src/index.ts",
  "src/profile.ts",
  "src/render.ts",
  "src/internal/continuation.ts",
  "src/internal/scope.ts",
  "src/internal/resource.ts",
];

export default defineConfig([
  {
    entry,
    platform: "neutral",
    unbundle: true,
    outDir: "dist",
    dts: { tsconfig: "./tsconfig.build.json" },
    define: { __REZE_HTML__: "false" },
  },
  {
    entry,
    platform: "neutral",
    unbundle: true,
    outDir: "dist/html",
    dts: false,
    define: { __REZE_HTML__: "true" },
  },
]);
