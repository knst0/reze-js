import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts", "src/profile.ts", "src/render.ts", "src/internal/continuation.ts"],
  platform: "neutral",
  unbundle: true,
  dts: { tsconfig: "./tsconfig.build.json" },
});
