import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts", "src/fs/index.ts"],
  platform: "neutral",
  unbundle: true,
  deps: { neverBundle: [/^node:/] },
  dts: { tsconfig: "./tsconfig.build.json" },
});
