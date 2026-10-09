import { defineConfig } from "tsdown";

export default defineConfig({
  entry: [
    "src/index.ts",
    "src/jsx-runtime.ts",
    "src/jsx-dev-runtime.ts",
    "src/internal/reactivity.ts",
    "src/internal/runtime.ts",
    "src/internal/html.ts",
    "src/internal/client.ts",
    "src/node.ts",
  ],
  platform: "neutral",
  unbundle: true,
  dts: { tsconfig: "./tsconfig.build.json" },
});
