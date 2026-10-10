import { defineConfig } from "vite-plus";

export default defineConfig({
  pack: {
    entry: ["src/index.ts", "src/router.ts"],
    platform: "neutral",
    unbundle: true,
    dts: { tsconfig: "./tsconfig.build.json" },
  },
});
