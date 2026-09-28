import fileRoutes from "@rezejs/router/vite";
import reze from "@rezejs/vite-plugin";

const variant = process.env.BENCH_VARIANT ?? "router";

export default {
  root: `src/${variant}`,
  build: { outDir: `../../dist/${variant}`, emptyOutDir: true, modulePreload: { polyfill: false } },
  plugins: variant === "router" ? [reze(), fileRoutes({ dir: "routes", dts: false })] : [reze()],
};
