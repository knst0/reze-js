import { basename, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { rolldown } from "rolldown";

const input = process.argv[2];
if (!input) throw new Error("usage: node benches/bundled.mjs <bench.ts>");
const file = join(import.meta.dirname, "..", "node_modules", ".bench", basename(input).replace(/\.ts$/, ".mjs"));
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
