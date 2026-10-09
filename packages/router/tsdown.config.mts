import { compile } from "@rezejs/compiler";
import { defineConfig, type Rolldown } from "tsdown";

export const rezeCompiler: Rolldown.Plugin = {
  name: "reze-compiler",
  transform(code, id) {
    if (id.includes("/node_modules/") || /\.d\.[cm]?ts$/.test(id) || !/\.[cm]?[jt]sx?$/.test(id)) return null;
    const result = compile(code, id, { sourceMap: true });
    if (result === null) return null;
    const errors = result.diagnostics.filter((diagnostic) => diagnostic.severity === "error");
    if (errors.length > 0) this.error(errors.map((diagnostic) => diagnostic.rendered).join("\n\n"));
    for (const diagnostic of result.diagnostics) {
      if (diagnostic.severity === "warn") this.warn(diagnostic.rendered);
    }
    return { code: result.code ?? code, map: result.map };
  },
};

export default defineConfig({
  entry: ["src/index.ts", "src/fs/index.ts", "src/internal/server.ts", "src/internal/swap.ts"],
  platform: "neutral",
  unbundle: true,
  deps: { neverBundle: [/^node:/] },
  plugins: [rezeCompiler],
  dts: { tsconfig: "./tsconfig.build.json" },
});
