import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { build, createServer } from "vite";
import { afterEach, beforeEach, expect, test } from "vite-plus/test";

import reze from "../src";

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "reze-spa-document-"));
  const modules = join(root, "node_modules");
  mkdirSync(modules, { recursive: true });
  for (const name of readdirSync(join(import.meta.dirname, "..", "node_modules"))) {
    symlinkSync(join(import.meta.dirname, "..", "node_modules", name), join(modules, name), "dir");
  }
  mkdirSync(join(root, "src"), { recursive: true });
  writeFileSync(
    join(root, "src", "document.tsx"),
    `import type { JSX } from "reze-js";

function Meta(props: { content: string }) {
  return <meta name="description" content={props.content} />;
}

export default function Document(props: { children: JSX.Element }) {
  return (
    <html lang="en">
      <head>
        <title>Acme</title>
        <Meta content="spa page" />
      </head>
      <body>
        <div id="app">{props.children}</div>
        <script type="module" src="/src/main.tsx"></script>
      </body>
    </html>
  );
}
`,
  );
  writeFileSync(
    join(root, "src", "main.tsx"),
    `import { render } from "reze-js";

function App() {
  return <p>spa</p>;
}

render(() => <App />, document.getElementById("app")!);
`,
  );
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

test("a client-only build renders its page from src/document.tsx", async () => {
  const outDir = join(root, "dist");
  await build({ root, configFile: false, logLevel: "silent", plugins: [reze()], build: { outDir, modulePreload: { polyfill: false } } });
  const html = readFileSync(join(outDir, "index.html"), "utf8");
  expect(html).toContain("<title>Acme</title>");
  expect(html).toContain('name=description content="spa page"');
  expect(html).toMatch(/<div id=app><\/div>/);
  expect(html).toMatch(/<script type="module" crossorigin src="\/assets\/index-[^"]+\.js"><\/script>/);
  expect(html).not.toContain("/@reze/client.js");
}, 30_000);

test("a leftover index.html next to the document fails the build", async () => {
  writeFileSync(join(root, "index.html"), "<!doctype html>");
  const failure = await build({
    root,
    configFile: false,
    logLevel: "silent",
    plugins: [reze()],
    build: { outDir: join(root, "dist"), modulePreload: { polyfill: false } },
  }).then(
    () => undefined,
    (error: unknown) => (error instanceof Error ? error.message : String(error)),
  );
  expect(failure).toMatch(/never reads index\.html/);
}, 30_000);

test("the dev server serves the compiled document for every route", async () => {
  const server = await createServer({
    root,
    configFile: false,
    logLevel: "silent",
    plugins: [reze()],
    server: { host: "127.0.0.1", port: 0 },
  });
  try {
    await server.listen();
    const url = server.resolvedUrls!.local[0]!;
    for (const path of ["", "about"]) {
      const html = await (await fetch(new URL(path, url), { headers: { accept: "text/html" } })).text();
      expect(html).toContain("<title>Acme</title>");
      expect(html).toContain('name=description content="spa page"');
      expect(html).toContain("/src/main.tsx");
      expect(html).toContain("/@vite/client");
    }
    const document = join(root, "src", "document.tsx");
    writeFileSync(document, readFileSync(document, "utf8").replace("<title>Acme</title>", "<title>Second</title>"));
    server.watcher.emit("change", document);
    const reloaded = await (await fetch(url, { headers: { accept: "text/html" } })).text();
    expect(reloaded).toContain("<title>Second</title>");
  } finally {
    await server.close();
  }
}, 30_000);
