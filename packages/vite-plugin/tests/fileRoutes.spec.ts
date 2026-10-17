import { mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { parse, type DefaultTreeAdapterTypes } from "parse5";
import { createServer } from "vite";
import { afterEach, beforeEach, expect, test } from "vite-plus/test";

import reze from "../src";
import { buildSsgFixture, expectBuildFails, listBuiltFiles } from "./ssg-harness";

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "reze-file-routes-"));
  mkdirSync(join(root, "src", "routes"), { recursive: true });
  symlinkSync(join(import.meta.dirname, "..", "node_modules"), join(root, "node_modules"), "dir");
  writeFileSync(join(root, "src", "routes", "index.tsx"), "export default function Home(){return <p>home</p>}");
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

test("route-specific extensions override the scan in a custom directory", async () => {
  const routes = join(root, "pages");
  mkdirSync(routes, { recursive: true });
  writeFileSync(join(routes, "index.tsx"), "export default function Home(){return <p>home</p>}");
  writeFileSync(join(routes, "guide.jsx"), "export default function Guide(){return <p>guide</p>}");
  const outDir = join(root, "dist");
  await buildSsgFixture({
    fixtureDir: root,
    outDir,
    plugins: await reze({ fileRoutes: { dir: "pages", extensions: [".tsx"], types: false }, ssg: true }),
  });
  expect(
    listBuiltFiles(outDir)
      .filter((file) => file.endsWith(".html"))
      .sort(),
  ).toEqual(["index.html"]);
  expect(paragraph(readFileSync(join(outDir, "index.html"), "utf8"))).toBe("home");
}, 30_000);

test("the document replaces index.html and refuses a dynamic shell", async () => {
  writeFileSync(join(root, "index.html"), "<!doctype html>");
  await expectBuildFails(reze({ fileRoutes: { types: false }, ssg: true }), root, join(root, "dist"), /never reads index\.html/);
  rmSync(join(root, "index.html"));
  writeFileSync(
    join(root, "src", "document.tsx"),
    'export default (props) => <html><head><title>{new Date().toString()}</title></head><body><div id="app">{props.children}</div></body></html>;',
  );
  await expectBuildFails(reze({ fileRoutes: { types: false }, ssg: true }), root, join(root, "dist"), /DOCUMENT_DYNAMIC/);
}, 30_000);

test("the dev server fills a new empty route file with a template and leaves written files alone", async () => {
  const routes = join(root, "src", "routes");
  const server = await createServer({
    configFile: false,
    root,
    logLevel: "silent",
    plugins: await reze({ fileRoutes: { types: false } }),
    server: { host: "127.0.0.1", port: 0, strictPort: true, hmr: false, watch: null },
  });
  try {
    mkdirSync(join(routes, "blog"));
    for (const file of ["blog.tsx", "blog/[id].tsx", "_parts.tsx", "about.tsx"]) {
      writeFileSync(join(routes, file), file === "about.tsx" ? "export default () => <p>about</p>;" : "");
      server.watcher.emit("add", join(routes, file));
    }
    expect(readFileSync(join(routes, "blog.tsx"), "utf8")).toContain("function BlogLayout(props: RouteProps)");
    expect(readFileSync(join(routes, "blog", "[id].tsx"), "utf8")).toContain('function BlogIdPage(props: RoutePropsFor<"/blog/:id">)');
    expect(readFileSync(join(routes, "_parts.tsx"), "utf8")).toBe("");
    expect(readFileSync(join(routes, "about.tsx"), "utf8")).toBe("export default () => <p>about</p>;");
  } finally {
    await server.close();
  }
}, 30_000);

test("template: false leaves a new empty route file empty", async () => {
  const server = await createServer({
    configFile: false,
    root,
    logLevel: "silent",
    plugins: await reze({ fileRoutes: { types: false, template: false } }),
    server: { host: "127.0.0.1", port: 0, strictPort: true, hmr: false, watch: null },
  });
  try {
    const file = join(root, "src", "routes", "about.tsx");
    writeFileSync(file, "");
    server.watcher.emit("add", file);
    expect(readFileSync(file, "utf8")).toBe("");
  } finally {
    await server.close();
  }
}, 30_000);
