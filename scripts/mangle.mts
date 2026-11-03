import { readdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

import { transform } from "esbuild";

const cacheFile = resolve(import.meta.dirname, "mangle.json");
const dist = resolve(process.argv[2] ?? "dist");

let cache = JSON.parse(await readFile(cacheFile, "utf8")) as Record<string, string | false>;
const files = (await readdir(dist, { recursive: true })).filter((file) => file.endsWith(".js")).sort();
for (const file of files) {
  const path = join(dist, file);
  const result = await transform(await readFile(path, "utf8"), { format: "esm", mangleProps: /^_/, mangleCache: cache, sourcefile: file });
  cache = result.mangleCache!;
  await writeFile(path, result.code);
}
const sorted = Object.fromEntries(Object.entries(cache).sort(([a], [b]) => a.localeCompare(b)));
await writeFile(cacheFile, `${JSON.stringify(sorted, null, 2)}\n`);
