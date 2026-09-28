import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { readResults, writeResults } from "../kit/results.mjs";
import { totalSize } from "../kit/sizes.mjs";

const here = import.meta.dirname;
const root = join(here, "..", "..");
const targets = JSON.parse(readFileSync(join(here, "targets.json"), "utf8"));
const previous = readResults(here)?.results ?? {};

const bytes = (n) => `${n} B`;
const delta = (current, was) =>
  was === undefined ? "" : ` (was ${bytes(was)}, ${current === was ? "same" : `${current > was ? "+" : ""}${current - was} B`})`;

const results = {};
for (const [pkg, target] of Object.entries(targets)) {
  execFileSync("pnpm", ["--filter", pkg, "build"], { cwd: root, stdio: "inherit" });
  const size = totalSize(join(root, target.dir, "dist"));
  const key = `${pkg} js`;
  const was = previous[key];
  const fields = ["raw", "gzip", "brotli"].map((field) => `${field} ${bytes(size[field])}${delta(size[field], was?.[field])}`);
  console.log(`${key}: ${fields.join(", ")}`);
  results[key] = size;
}

writeResults(here, { results });
