import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts", "src/jsx-runtime.ts"],
  platform: "neutral",
  unbundle: true,
  dts: true,
});
