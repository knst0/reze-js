import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";

const variant = process.env.BENCH_VARIANT ?? "router";

export default {
  root: `src/${variant}`,
  build: { outDir: `../../dist/${variant}`, emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins:
    variant === "router"
      ? [
          tanstackRouter({
            target: "react",
            autoCodeSplitting: true,
            routesDirectory: "routes",
            generatedRouteTree: "routeTree.gen.js",
            disableTypes: true,
          }),
          react(),
        ]
      : [react()],
};
