import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts", "src/vite/index.ts"],
  platform: "neutral",
  unbundle: true,
  deps: { neverBundle: [/^node:/] },
  dts: { tsconfig: "./tsconfig.build.json" },
});
