import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { pathToFileURL } from "node:url";

import { compileProject, type Diagnostic } from "@rezejs/compiler";
import { createApp } from "reze-js/runtime/server";
import { expect, test } from "vite-plus/test";

const root = import.meta.dirname;
const out = join(root, ".out");

function sources(dir: string, base = dir): Record<string, string> {
  const files: Record<string, string> = {};
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) Object.assign(files, sources(path, base));
    else if (/\.tsx?$/.test(entry.name)) {
      const name = relative(base, path);
      files[name === "input.tsx" ? "src/routes/index.tsx" : `src/${name}`] = readFileSync(path, "utf8");
    }
  }
  return files;
}

function format(diagnostic: Diagnostic): string {
  const lines = [diagnostic.rendered.trimEnd()];
  for (const fix of diagnostic.fixes) {
    for (const edit of fix.edits) lines.push(`  ${edit.start}..${edit.end} ${JSON.stringify(edit.text)}`);
  }
  return lines.join("\n");
}

async function snapshot(dir: string, name: string, content: string | undefined) {
  const path = join(dir, name);
  if (content === undefined) {
    expect(existsSync(path), `${name} expects output the fixture does not produce`).toBe(false);
  } else {
    await expect(content).toMatchFileSnapshot(path);
  }
}

const fixtures = readdirSync(root, { withFileTypes: true }).filter(
  (entry) => entry.isDirectory() && !entry.name.startsWith(".") && entry.name !== "node_modules",
);

for (const fixture of fixtures) {
  test(fixture.name, async () => {
    const dir = join(root, fixture.name);
    const result = compileProject({ files: sources(dir) });
    const diagnostics = result.diagnostics.map(format).join("\n\n");
    await snapshot(dir, "diagnostics.snap", diagnostics ? `${diagnostics}\n` : undefined);
    if (result.diagnostics.some((d) => d.severity === "error")) {
      await snapshot(dir, "html.snap", undefined);
      return;
    }
    const target = join(out, fixture.name);
    rmSync(target, { recursive: true, force: true });
    for (const [path, module] of Object.entries(result.modules)) {
      if (module.server === undefined || module.server === null) continue;
      mkdirSync(dirname(join(target, path)), { recursive: true });
      writeFileSync(join(target, path), module.server);
    }
    writeFileSync(join(target, "app.ts"), result.app);
    const client = [
      ...Object.entries(result.modules)
        .sort(([a], [b]) => a.localeCompare(b))
        .flatMap(([path, module]) => (module.client ? [`// ${path}\n${module.client}`] : [])),
      ...Object.entries(result.chunks)
        .sort(([a], [b]) => Number(a) - Number(b))
        .map(([node, code]) => `// virtual:rz-route/${node}\n${code}`),
    ];
    await snapshot(dir, "client.snap", client.length > 0 ? client.join("\n") : undefined);
    const routes = (await import(pathToFileURL(join(target, "app.ts")).href)).default;
    const assets = { dispatcher: "/dispatcher.js", chunk: (node: number) => `/route-${node}.js` };
    const response = await createApp(routes, { assets })(new Request("http://localhost/"));
    await snapshot(dir, "html.snap", `${await response.text()}\n`);
  });
}
