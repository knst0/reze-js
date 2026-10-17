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

test("the document replaces index.html", async () => {
  writeFileSync(join(root, "index.html"), "<!doctype html>");
  await expectBuildFails(reze({ fileRoutes: { types: false }, ssg: true }), root, join(root, "dist"), /never reads index\.html/);
}, 30_000);

function head(html: string): string {
  return html.slice(html.indexOf("<head>"), html.indexOf("</head>"));
}

test("the document inlines imported components and renders the rest by running it once", async () => {
  writeFileSync(
    join(root, "src", "preload.tsx"),
    'export const Font = "/inter.woff2";\nexport function Preload(props) { return <link rel="preload" as="font" href={props.href} crossorigin="" />; }',
  );
  writeFileSync(
    join(root, "src", "document.tsx"),
    'import { Font, Preload } from "./preload";\nexport default (props) => <html lang="en"><head><title>Acme</title><Preload href={Font} /></head><body><main id="app">{props.children}</main></body></html>;',
  );
  const outDir = join(root, "dist");
  await buildSsgFixture({ fixtureDir: root, outDir, plugins: reze({ fileRoutes: { types: false }, ssg: true }) });
  const page = readFileSync(join(outDir, "index.html"), "utf8");
  expect(head(page)).toContain('<link rel="preload" as="font" href="/inter.woff2" crossorigin="">');
  expect(paragraph(page)).toBe("home");

  writeFileSync(
    join(root, "src", "document.tsx"),
    'import { Preload } from "./preload";\nconst built = new Date(0).toISOString();\nexport default (props) => <html lang="en"><head><title>{built}</title><Preload href={"/" + "inter.woff2"} /></head><body><main id="app">{props.children}</main></body></html>;',
  );
  await buildSsgFixture({ fixtureDir: root, outDir, plugins: reze({ fileRoutes: { types: false }, ssg: true }) });
  const rendered = readFileSync(join(outDir, "index.html"), "utf8");
  expect(head(rendered)).toContain(">1970-01-01T00:00:00.000Z</title>");
  expect(head(rendered)).toContain('href="/inter.woff2"');
  expect(paragraph(rendered)).toBe("home");
  expect(listBuiltFiles(outDir).filter((file) => file.includes("reze-document"))).toEqual([]);
}, 60_000);

test("the dev server renders a document it cannot compile and rerenders it on change", async () => {
  const document = join(root, "src", "document.tsx");
  const source = (title: string) =>
    `const title = [${JSON.stringify(title)}].join("");\nexport default (props) => <html><head><title>{title}</title></head><body><div id="app">{props.children}</div></body></html>;`;
  writeFileSync(document, source("first"));
  const server = await createServer({
    configFile: false,
    root,
    logLevel: "silent",
    plugins: await reze({ fileRoutes: { types: false }, ssg: true }),
    server: { host: "127.0.0.1", port: 0 },
  });
  try {
    await server.listen();
    const url = server.resolvedUrls!.local[0]!;
    const page = await (await fetch(url, { headers: { accept: "text/html" } })).text();
    expect(head(page)).toContain(">first</title>");
    expect(paragraph(page)).toBe("home");
    writeFileSync(document, source("second"));
    server.watcher.emit("change", document);
    await new Promise((resolve) => setTimeout(resolve, 200));
    const next = await (await fetch(url, { headers: { accept: "text/html" } })).text();
    expect(head(next)).toContain(">second</title>");
  } finally {
    await server.close();
  }
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
