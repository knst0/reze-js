import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { parse, type DefaultTreeAdapterTypes } from "parse5";
import { afterEach, beforeEach, expect, test } from "vitest";

import reze, { type FileRoutesOptions } from "../src";
import { buildSsgFixture, listBuiltFiles } from "./ssg-harness";

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "reze-file-routes-"));
  mkdirSync(join(root, "src"));
  symlinkSync(join(import.meta.dirname, "..", "node_modules"), join(root, "node_modules"), "dir");
  writeFileSync(join(root, "index.html"), '<!doctype html><div id="app"></div><script type="module" src="/@reze/ssg-client.js"></script>');
  writeFileSync(join(root, "src", "app.tsx"), 'export { routes, paths } from "virtual:reze-routes";');
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

function paragraph(html: string): string {
  const queue: DefaultTreeAdapterTypes.Node[] = [parse(html)];
  while (queue.length > 0) {
    const node = queue.pop()!;
    if (!("childNodes" in node)) continue;
    if (node.nodeName === "p") return node.childNodes.map((child) => ("value" in child ? child.value : "")).join("");
    queue.push(...node.childNodes);
  }
  throw new Error("missing rendered paragraph");
}

interface RouteSelection {
  name: string;
  extensions?: string[];
  fileRoutes: FileRoutesOptions;
  pages: readonly (readonly [string, string])[];
}

const selections: RouteSelection[] = [
  {
    name: "top-level extensions replace the route scan",
    extensions: [".jsx"],
    fileRoutes: { types: false },
    pages: [["guide/index.html", "guide"]],
  },
  {
    name: "route-specific extensions override the scan in a custom directory",
    fileRoutes: { dir: "pages", extensions: [".tsx"], types: false },
    pages: [["index.html", "home"]],
  },
];

test.each(selections)(
  "$name",
  async ({ extensions, fileRoutes, pages }) => {
    const routes = join(root, fileRoutes.dir ?? "src/routes");
    mkdirSync(routes, { recursive: true });
    writeFileSync(join(routes, "index.tsx"), "export default function Home(){return <p>home</p>}");
    writeFileSync(join(routes, "guide.jsx"), "export default function Guide(){return <p>guide</p>}");
    const outDir = join(root, "dist");
    await buildSsgFixture({
      fixtureDir: root,
      outDir,
      plugins: await reze({ extensions, fileRoutes, ssg: { entry: "src/app.tsx" } }),
    });
    expect(
      listBuiltFiles(outDir)
        .filter((file) => file.endsWith(".html"))
        .sort(),
    ).toEqual(pages.map(([file]) => file).sort());
    for (const [file, expected] of pages) expect(paragraph(readFileSync(join(outDir, file), "utf8"))).toBe(expected);
  },
  30_000,
);
