import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts", "src/devtools.ts", "src/render.ts"],
  platform: "neutral",
  unbundle: true,
  dts: { tsconfig: "./tsconfig.build.json" },
});
