import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { build, createServer } from "vite";
import { expect, test } from "vite-plus/test";

import llms from "./llms";

const page = (title: string, body: string) => `---\ntitle: ${title}\ndescription: A useful guide.\n---\n\n# ${title}\n\n${body}\n`;

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), "reze-docs-"));
  await mkdir(join(root, "src/routes/(guides)/nested"), { recursive: true });
  await writeFile(join(root, "index.html"), "<!doctype html><title>Docs</title>");
  return root;
}

test("builds grouped and index routes as Markdown, rewrites document links, and preserves fenced JSX", async () => {
  const root = await fixture();
  try {
    await writeFile(join(root, "src/routes/(guides)/nested/index.mdx"), page("Nested", "Read [intro](/intro?q=1#start)."));
    await writeFile(
      join(root, "src/routes/intro.mdx"),
      page(
        "Introduction",
        [
          'import Demo from "./Demo";',
          "",
          "<Aside>Keep **this text**.</Aside>",
          "",
          "[nested](./nested#details) and [external](https://example.com/nested).",
          "",
          "```tsx",
          'import { signal } from "reze-js";',
          "const view = <Demo />;",
          "```",
        ].join("\n"),
      ),
    );
    await writeFile(join(root, "src/routes/_private.mdx"), "Not a route");
    await build({ root, configFile: false, base: "/docs/", plugins: [llms()], logLevel: "silent" });
    const index = await readFile(join(root, "dist/llms.txt"), "utf8");
    expect(index).toContain("[Introduction](/docs/intro.md): A useful guide.");
    expect(index).toContain("[Nested](/docs/nested.md)");
    expect(index).not.toContain("private");
    const intro = await readFile(join(root, "dist/intro.md"), "utf8");
    expect(intro).toContain("Keep **this text**.");
    expect(intro).toContain("[nested](/docs/nested.md#details)");
    expect(intro).toContain("[external](https://example.com/nested)");
    expect(intro).toContain('```tsx\nimport { signal } from "reze-js";\nconst view = <Demo />;\n```');
    expect(intro).not.toContain("import Demo");
    expect(intro).not.toContain("<Aside>");
    expect(intro).not.toContain("description:");
    expect(await readFile(join(root, "dist/nested.md"), "utf8")).toContain("[intro](/docs/intro.md?q=1#start)");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("serves fresh Markdown on GET and HEAD, returns 404 for missing pages, and diagnoses invalid metadata", async () => {
  const root = await fixture();
  const file = join(root, "src/routes/intro.mdx");
  const server = await createServer({
    root,
    configFile: false,
    base: "/docs/",
    plugins: [llms()],
    logLevel: "silent",
    server: { port: 0 },
  });
  try {
    await writeFile(file, page("Introduction", "Initial content."));
    await server.listen();
    const address = server.httpServer!.address();
    if (!address || typeof address === "string") throw new Error("Expected a TCP server");
    const url = `http://localhost:${address.port}/docs/`;
    const response = await fetch(`${url}intro.md?raw`);
    expect(response.headers.get("content-type")).toBe("text/markdown; charset=utf-8");
    expect(await response.text()).toContain("Initial content.");
    await writeFile(file, page("Updated", "Changed content."));
    expect(await (await fetch(`${url}intro.md`)).text()).toContain("Changed content.");
    expect(await (await fetch(`${url}llms.txt`)).text()).toContain("[Updated](/docs/intro.md)");
    const head = await fetch(`${url}intro.md`, { method: "HEAD" });
    expect(head.status).toBe(200);
    expect(await head.text()).toBe("");
    expect((await fetch(`${url}missing.md`)).status).toBe(404);
    await writeFile(file, "---\ntitle: Missing description\n---\n# Invalid\n");
    await expect(build({ root, configFile: false, plugins: [llms()], logLevel: "silent" })).rejects.toThrow(
      "frontmatter requires non-empty title and description strings",
    );
  } finally {
    await server.close();
    await rm(root, { recursive: true, force: true });
  }
});
