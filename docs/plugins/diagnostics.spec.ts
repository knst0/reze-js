import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import type { Nodes, Table } from "mdast";
import { toString } from "mdast-util-to-string";
import { expect, test } from "vite-plus/test";

import { readPages } from "./pages";

const compilerDir = dirname(fileURLToPath(import.meta.resolve("@rezejs/compiler")));
const repairGuide = await readFile(join(compilerDir, "skills/reze-compiler-diagnostics/SKILL.md"), "utf8");
const catalogCodes = [...repairGuide.matchAll(/^## ([A-Z][A-Z_]+)$/gm)].map((match) => match[1]).sort();

function referenceTables(root: Nodes): Table[] {
  if (!("children" in root)) return [];
  const tables: Table[] = [];
  let inReference = false;
  for (const node of root.children) {
    if (node.type === "heading") inReference = toString(node) === "Diagnostic reference";
    else if (inReference && node.type === "table") tables.push(node);
  }
  return tables;
}

test("the compiler page's diagnostic reference lists exactly the catalog's codes", async () => {
  const pages = await readPages(join(import.meta.dirname, "../src/routes"));
  const compilerPage = pages.find((page) => page.name === "compiler");
  if (compilerPage === undefined) throw new Error("docs/src/routes/compiler.mdx is missing");
  const documented = referenceTables(compilerPage.tree)
    .flatMap((table) => table.children.slice(1))
    .map((row) => toString(row.children[0] ?? { type: "text", value: "" }))
    .sort();
  expect(catalogCodes.length).toBeGreaterThan(0);
  expect(documented).toEqual(catalogCodes);
});
