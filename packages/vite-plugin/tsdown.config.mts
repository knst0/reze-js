import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts", "src/manifest-cli.ts"],
  platform: "node",
  fixedExtension: false,
  unbundle: true,
  dts: { tsconfig: "./tsconfig.build.json" },
});
