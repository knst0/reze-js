import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parse } from "parse5";
import type { DefaultTreeAdapterMap } from "parse5";
import { afterEach, beforeEach, expect, test } from "vitest";

import { buildClientRegistry, decideAssetImport } from "../src/ssg/assets";
import { resolveAppMode } from "../src/ssg/export-graph";
import { resolvePathsCallbacks, resolveSsgOptions } from "../src/ssg/options";
import { bootstrapScriptSrc, buildPage, buildRedirectPage, countRootIds, readHeadDefaults, validateTemplate } from "../src/ssg/template";
import { canonicalPageUrl, joinBase, normalizePageUrl, outputFileFor, planOutputs } from "../src/ssg/urls";

let dir: string;

function elements(parent: DefaultTreeAdapterMap["parentNode"]): DefaultTreeAdapterMap["element"][] {
  return parent.childNodes.flatMap(node => "tagName" in node ? [node, ...elements(node)] : []);
}

function attribute(node: DefaultTreeAdapterMap["element"], name: string): string | undefined {
  return node.attrs.find(attr => attr.name === name)?.value;
}

function text(node: DefaultTreeAdapterMap["parentNode"]): string {
  return node.childNodes.map(child => "value" in child ? child.value : "childNodes" in child ? text(child) : "").join("");
}

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "reze-ssg-units-"));
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});


test("ssg options reject a missing entry, a loose selector and a dead timeout", () => {
  expect(() => resolveSsgOptions({ entry: "" }, "/root")).toThrow(/ssg\.entry/);
  expect(() => resolveSsgOptions({ entry: "src/app.tsx", selector: ".app" as `#${string}` }, "/root")).toThrow(/selector/);
  expect(() => resolveSsgOptions({ entry: "src/app.tsx", selector: "#a b" as `#${string}` }, "/root")).toThrow(/selector/);
  expect(() => resolveSsgOptions({ entry: "src/app.tsx", timeoutMs: 0 }, "/root")).toThrow(/timeoutMs/);
  expect(() => resolveSsgOptions({ entry: "src/app.tsx", timeoutMs: Number.NaN }, "/root")).toThrow(/timeoutMs/);
  expect(() => resolveSsgOptions({ entry: "src/app.tsx", trailingSlash: "sometimes" as "always" }, "/root")).toThrow(/trailingSlash/);
});

test("paths callbacks resolve once and validate every param shape", async () => {
  const resolved = await resolvePathsCallbacks({
    "/blog/:id": [{ id: "a" }, { id: "b" }],
    "/files/*path": () => [{ path: ["x", "y"] }],
  });
  expect(resolved).toEqual({ "/blog/:id": [{ id: "a" }, { id: "b" }], "/files/*path": [{ path: ["x", "y"] }] });
  await expect(resolvePathsCallbacks({ "/x": ["nope"] as unknown as never })).rejects.toThrow(/must be a record/);
  await expect(resolvePathsCallbacks({ "/x": [{ id: 3 as unknown as string }] })).rejects.toThrow(/must be a string/);
});

test("the template requires one bootstrap script and one mount root", () => {
  const html = `<html><head></head><body><div id="app"></div><script type="module" src="/@reze/ssg-client.js"></script></body></html>`;
  validateTemplate(html, "index.html", "app", "/@reze/ssg-client.js");
  expect(() => validateTemplate(html.replace("ssg-client", "other"), "index.html", "app", "/@reze/ssg-client.js")).toThrow(/exactly one/);
  expect(() => validateTemplate(`${html}<script type="module" src="/@reze/ssg-client.js"></script>`, "index.html", "app", "/@reze/ssg-client.js")).toThrow(/exactly one/);
  expect(() => validateTemplate(html.replace('id="app"', 'id="root"'), "index.html", "app", "/@reze/ssg-client.js")).toThrow(/mount|id/);
});

test("page assembly preserves nested roots, raw text and decoded head values", () => {
  const templateHtml = `<html><head><title>Base &amp; title</title><meta name=description content="base &amp; description"><script>globalThis.fake = '<div id="app"></div>';</script></head><body><!-- <div id=app></div> --><template><div id=app></div></template><div id=app data-note=">"><div>old</div><div>old second</div></div><aside id=after>kept</aside><script type=module src=./assets/boot.js></script></body></html>`;
  validateTemplate(templateHtml, "index.html", "app", "./assets/boot.js");
  expect(countRootIds(templateHtml, "app")).toBe(1);
  const baseline = readHeadDefaults(templateHtml);
  expect(baseline).toEqual({ title: "Base & title", description: "base & description" });
  const payload = { message: "</script><script>alert(1)</script>&\u2028\u2029" };
  const page = buildPage({
    templateHtml, templateFile: "index.html", baseline, rootId: "app", base: "./", pathname: "/a/b/",
    metadata: { title: "Page $& <x>" }, content: "<p>new</p>", payload: JSON.stringify(payload),
    portals: [],
    assets: { css: ["../../assets/a.css", "../../assets/b.css"], js: ["../../assets/shared.js"] },
  });
  const nodes = elements(parse(page));
  const root = nodes.find(node => attribute(node, "id") === "app")!;
  expect(text(root)).toBe("new");
  expect(text(nodes.find(node => attribute(node, "id") === "after")!)).toBe("kept");
  expect(text(nodes.find(node => node.tagName === "title")!)).toBe("Page $& <x>");
  expect(attribute(nodes.find(node => attribute(node, "name") === "description")!, "content")).toBe("base & description");
  const scripts = nodes.filter(node => node.tagName === "script");
  expect(scripts).toHaveLength(3);
  expect(text(scripts[0]!)).toBe(`globalThis.fake = '<div id="app"></div>';`);
  expect(attribute(scripts[1]!, "src")).toBe("../../assets/boot.js");
  expect(JSON.parse(text(scripts[2]!))).toEqual(payload);
  expect(nodes.filter(node => node.tagName === "link").map(node => [attribute(node, "rel"), attribute(node, "href")]))
    .toEqual([["stylesheet", "../../assets/a.css"], ["stylesheet", "../../assets/b.css"], ["modulepreload", "../../assets/shared.js"]]);
});

test("the built bootstrap is matched by file name, not by guesswork", () => {
  const html = `<html><head></head><body><script type="module" src="/docs/assets/ssg-abc123.js"></script><script type="module" src="https://cdn.example/x.js"></script></body></html>`;
  expect(bootstrapScriptSrc(html, "assets/ssg-abc123.js")).toBe("/docs/assets/ssg-abc123.js");
  expect(() => bootstrapScriptSrc(html, "assets/missing.js")).toThrow(/bootstrap/);
});

test("redirect pages drop the bootstrap and carry an accessible target", () => {
  const html = `<html><head><title>T</title></head><body><div id="app"></div><script type="module" src="/docs/assets/ssg-abc.js"></script></body></html>`;
  const to = "/b/?query=</script>&\u2028\u2029";
  const page = buildRedirectPage({
    templateHtml: html, templateFile: "index.html", baseline: {}, canonical: to, to,
    replace: true, rootId: "app", base: "/docs/", pathname: "/old/", redirectSrc: "/docs/assets/redirect-def.js",
  });
  const nodes = elements(parse(page));
  expect(nodes.filter(node => node.tagName === "script").map(node => attribute(node, "src")))
    .toEqual([undefined, "/docs/assets/redirect-def.js"]);
  expect(attribute(nodes.find(node => node.tagName === "a")!, "href")).toBe(to);
  expect(attribute(nodes.find(node => attribute(node, "http-equiv") === "refresh")!, "content")).toBe(`0;url=${to}`);
  expect(JSON.parse(text(nodes.find(node => attribute(node, "data-reze-redirect") === "app")!))).toEqual({ to, replace: true });
});


test("the export graph tells router apps from standalone ones", () => {
  const files = new Map([
    ["/r/app.tsx", `export { routes } from "./routes";\nexport { default } from "./shell";\n`],
    ["/r/routes.ts", `export const routes = [];\n`],
    ["/r/shell.tsx", `export default function Shell() {}\n`],
    ["/s/app.tsx", `export default function App() {}\n`],
  ]);
  const env = {
    readFile: (id: string) => files.get(id),
    resolveSpec: (spec: string, importer: string) => {
      if (spec === "./routes") return "/r/routes.ts";
      if (spec === "./shell") return "/r/shell.tsx";
      void importer;
      return undefined;
    },
    virtualExports: (_id: string) => undefined as readonly string[] | undefined,
  };
  expect(resolveAppMode("/r/app.tsx", env)).toEqual({ kind: "router", hasShell: true });
  expect(resolveAppMode("/s/app.tsx", env)).toEqual({ kind: "standalone" });
});

test("the export graph follows export stars and rejects ambiguities", () => {
  const files = new Map([
    ["/e/entry.ts", `export * from "./a";\nexport * from "./b";\n`],
    ["/e/a.ts", `export const routes = [];\n`],
    ["/e/b.ts", `export const routes = [];\n`],
    ["/f/entry.ts", `export * from "./a";\nexport const routes = [];\n`],
  ]);
  const env = {
    readFile: (id: string) => files.get(id),
    resolveSpec: (spec: string, importer: string) => (spec === "./a" ? "/e/a.ts" : spec === "./b" ? "/e/b.ts" : importer),
    virtualExports: (_id: string) => undefined as readonly string[] | undefined,
  };
  expect(() => resolveAppMode("/e/entry.ts", env)).toThrow(/ambiguous/);
  expect(resolveAppMode("/f/entry.ts", env)).toEqual({ kind: "router", hasShell: false });
});

test("page urls reject traversal, encoded slashes and reserved segments", () => {
  expect(normalizePageUrl("/a/b")).toBe("/a/b");
  expect(normalizePageUrl("/a/./b/")).toBe("/a/b");
  expect(normalizePageUrl("/a?x=1#h")).toBe("/a");
  expect(() => normalizePageUrl("relative")).toThrow(/absolute/);
  expect(() => normalizePageUrl("/../etc")).toThrow(/traverse/);
  expect(() => normalizePageUrl("/a%2Fb")).toThrow(/slash/);
  expect(() => normalizePageUrl("/a%5Cb")).toThrow(/slash/);
  expect(() => normalizePageUrl("/con/x")).toThrow(/reserved/);
  expect(canonicalPageUrl("/a/b", "always")).toBe("/a/b/");
  expect(canonicalPageUrl("/a/b", "never")).toBe("/a/b");
  expect(canonicalPageUrl("/", "never")).toBe("/");
  expect(outputFileFor("/")).toBe("index.html");
  expect(outputFileFor("/a/b")).toBe("a/b/index.html");
  expect(() => planOutputs(["/a/", "/a"])).toThrow(/duplicate/);
});

test("asset links respect absolute, origin and relative bases", () => {
  expect(joinBase("/", "assets/a.js", 2)).toBe("/assets/a.js");
  expect(joinBase("https://cdn.example/s", "assets/a.js", 1)).toBe("https://cdn.example/s/assets/a.js");
  expect(joinBase("./", "assets/a.js", 0)).toBe("./assets/a.js");
  expect(joinBase("./", "assets/a.js", 2)).toBe("../../assets/a.js");
});

test("asset imports decide between raw, inline and registry lookup", () => {
  const small = join(dir, "small.png");
  const big = join(dir, "big.png");
  writeFileSync(small, "x".repeat(10));
  writeFileSync(big, "x".repeat(10_000));
  const include = (id: string) => id.endsWith(".png");
  expect(decideAssetImport({ file: small, query: "", assetsInclude: include, inlineLimit: 4096 })).toBe("inline");
  expect(decideAssetImport({ file: big, query: "", assetsInclude: include, inlineLimit: 4096 })).toBe("lookup");
  expect(decideAssetImport({ file: big, query: "raw", assetsInclude: include, inlineLimit: 4096 })).toBe("raw");
  expect(decideAssetImport({ file: big, query: "inline", assetsInclude: include, inlineLimit: 4096 })).toBe("inline");
  expect(decideAssetImport({ file: join(dir, "note.txt"), query: "", assetsInclude: include, inlineLimit: 4096 })).toBe("passthrough");
  expect(decideAssetImport({ file: join(dir, "gone.png"), query: "", assetsInclude: include, inlineLimit: 4096 })).toBe("lookup");
});

test("the client registry maps canonical assets and selects page closures", () => {
  const registry = buildClientRegistry([
    { type: "asset", fileName: "assets/a-1.png", originalFileName: "src/a.png" },
    {
      type: "chunk", fileName: "assets/view-1.js", isEntry: false, facadeModuleId: null,
      moduleIds: ["\0reze:ssg-view.tsx"], imports: ["assets/shared-1.js"], dynamicImports: ["assets/route-1.js"],
      viteMetadata: { importedCss: ["assets/view-1.css"] },
    },
    {
      type: "chunk", fileName: "assets/shared-1.js", isEntry: false, facadeModuleId: null,
      moduleIds: ["/root/src/shared.ts"], imports: [], dynamicImports: [], viteMetadata: {},
    },
    {
      type: "chunk", fileName: "assets/route-1.js", isEntry: false, facadeModuleId: null,
      moduleIds: ["/root/src/routes/post.tsx"], imports: [], dynamicImports: [],
      viteMetadata: { importedCss: ["assets/route-1.css"] },
    },
    {
      type: "chunk", fileName: "assets/boot-1.js", isEntry: true, facadeModuleId: "\0reze:ssg-client.js",
      moduleIds: ["\0reze:ssg-client.js"], imports: ["assets/view-1.js"], dynamicImports: [], viteMetadata: {},
    },
  ], "/root");
  expect(registry.assetFile("src/a.png")).toBe("assets/a-1.png");
  expect(registry.entryChunk("\0reze:ssg-client.js").fileName).toBe("assets/boot-1.js");
  expect(() => registry.assetFile("src/missing.png")).toThrow(/no client output/);
  const closure = registry.staticClosure(["assets/view-1.js", "assets/route-1.js"]);
  expect(closure.js).toEqual(["assets/view-1.js", "assets/route-1.js", "assets/shared-1.js"]);
  expect(closure.css).toEqual(["assets/view-1.css", "assets/route-1.css"]);
  expect(registry.chunkFileForModule("/root/src/routes/post.tsx")).toBe("assets/route-1.js");
});
