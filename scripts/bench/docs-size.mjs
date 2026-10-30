import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, posix, relative } from "node:path";

import { measure, recordSnapshot, sumSizes } from "./sizes.mjs";

const docsDir = join(import.meta.dirname, "..", "..", "docs");
const distDir = join(docsDir, "dist");
const resultsPath = join(docsDir, "size.json");

const staticImport = /(?:^|[;}\s])import\s*(?:[\w$*{},\s]+from\s*)?["'](\.\/[^"']+\.js)["']/g;

function pageEntries(html) {
  const entries = [];
  for (const [tag] of html.matchAll(/<(?:script|link)\b[^>]*>/g)) {
    const src = /\b(?:src|href)="?([^"\s>]+\.js)/.exec(tag)?.[1];
    if (src && (tag.startsWith("<script") || tag.includes("modulepreload"))) entries.push(src);
  }
  for (const [, module] of html.matchAll(/"m":"([^"]+\.js)"/g)) entries.push(module);
  return entries.map((src) => src.replace(/^\//, ""));
}

function initialChunks(html) {
  const loaded = new Set();
  const pending = pageEntries(html);
  while (pending.length > 0) {
    const file = pending.pop();
    if (loaded.has(file)) continue;
    loaded.add(file);
    for (const [, specifier] of readFileSync(join(distDir, file), "utf8").matchAll(staticImport)) {
      pending.push(posix.join(posix.dirname(file), specifier));
    }
  }
  return [...loaded];
}

const pages = readdirSync(distDir, { recursive: true })
  .filter((file) => file.endsWith("index.html"))
  .map((file) => ({
    route: `/${relative(distDir, join(distDir, dirname(file))).replaceAll("\\", "/")}`,
    html: readFileSync(join(distDir, file), "utf8"),
  }))
  .filter(({ html }) => !html.includes('http-equiv="refresh"'));

const sizes = {};
for (const { route, html } of pages) {
  sizes[route] = sumSizes(initialChunks(html).map((file) => measure(readFileSync(join(distDir, file)))));
}

console.log("docs: initial JS bytes per prerendered page");
console.table(sizes);
recordSnapshot(resultsPath, sizes, { check: process.argv.includes("--check") });
