import { createRequire } from "node:module";
import { basename, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const input = process.argv[2];
if (!input) throw new Error("usage: node benches/kit/bundled.mjs <bench.ts>");
const root = join(import.meta.dirname, "..", "..");
const file = join(root, "packages", "signals", "node_modules", ".bench", basename(input).replace(/\.ts$/, ".mjs"));
const require = createRequire(join(root, "packages", "signals", "package.json"));
const { rolldown } = await import(pathToFileURL(require.resolve("rolldown")).href);
const build = await rolldown({
  input: resolve(input),
  platform: "node",
  external: ["tinybench"],
  transform: {
    define: {
      "process.env.NODE_ENV": '"production"',
      __REZE_HTML__: "false",
      __REZE_HYDRATE__: "false",
    },
  },
});
await build.write({ file, format: "esm", minify: true });
await build.close();
await import(pathToFileURL(file).href);
