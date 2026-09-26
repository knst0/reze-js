import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts", "src/jsx-runtime.ts"],
  dts: { tsconfig: "../../tsconfig.json" },
  platform: "neutral",
  unbundle: true,
  dts: true,
});
