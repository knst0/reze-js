import { existsSync } from "node:fs";
import { mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { createBuilder } from "vite";

import reze from "../../packages/vite-plugin/src/index.ts";
import { sourceAliases } from "../../vitest.shared.ts";
import { measure, readSnapshot, recordSnapshot, type Size } from "./sizes.ts";

const root = resolve(import.meta.dirname, "..", "..");
const apps = join(root, "tests", "apps");
const results = join(import.meta.dirname, "size.json");
/** The published build of each package, so the sizes include its mangled properties. */
const distAliases = sourceAliases.map(({ find, replacement }) => ({
  find,
  replacement: replacement.replace(/([/\\])src([/\\])/, "$1dist$2").replace(/\.ts$/, ".js"),
}));
const metadata = [
  / data-rz(?:-prevent)?=(?:"[^"]*"|[^\s>]+)/g,
  /<!--[0-9a-z]+\.[0-9a-z]+-->/g,
  /<!>/g,
  /<script type="rz">.*?<\/script>/gs,
];
/** The pages measured besides `/`, for the applications sized like a real site. */
const pages: Record<string, string[]> = { shop: ["/products/8"] };

async function build(app: string, out: string): Promise<(request: Request) => Promise<Response>> {
  const builder = await createBuilder({
    root: app,
    configFile: false,
    logLevel: "warn",
    plugins: [reze()],
    resolve: { alias: distAliases },
    environments: {
      client: { build: { outDir: join(out, "client"), emptyOutDir: true } },
      ssr: { build: { outDir: join(out, "server"), emptyOutDir: true } },
    },
  });
  await builder.buildApp();
  const [entry] = (await readdir(join(out, "server"))).filter((name) => /^server\.m?js$/.test(name));
  const server = await import(pathToFileURL(join(out, "server", entry!)).href);
  return "createHandler" in server ? server.createHandler() : server.default;
}

const sizes: Record<string, Size> = {};
const rows: Record<string, string | number>[] = [];
for (const app of (await readdir(apps, { withFileTypes: true }))
  .filter((entry) => entry.isDirectory() && entry.name !== "spa" && !existsSync(join(apps, entry.name, "Cargo.toml")))
  .map((entry) => entry.name)
  .sort()) {
  const out = await mkdtemp(join(tmpdir(), "rz-size-"));
  await writeFile(join(out, "package.json"), '{ "type": "module" }');
  try {
    const handler = await build(join(apps, app), out);
    for (const path of ["/", ...(pages[app] ?? [])]) {
      const html = await (await handler(new Request(`http://localhost${path}`))).text();
      const bare = metadata.reduce((text, pattern) => text.replace(pattern, ""), html);
      const page = measure(html);
      const bytes = page.brotli - measure(bare).brotli;
      const file = path === "/" ? `${app}: html` : `${app}: html ${path}`;
      sizes[file] = page;
      rows.push({ file, ...page, metadata: `${((bytes / page.brotli) * 100).toFixed(1)}%, ${bytes} B` });
    }
    const assets = join(out, "client", "assets");
    const files = await readdir(assets).catch(() => [] as string[]);
    for (const file of files.sort()) {
      const name = `${app}: ${file.replace(/-[\w-]{8}\.js$/, ".js")}`;
      sizes[name] = measure(await readFile(join(assets, file)));
      rows.push({ file: name, ...sizes[name] });
    }
  } finally {
    await rm(out, { recursive: true, force: true });
  }
}

const previous = readSnapshot(results);
for (const row of rows) {
  const committed = previous[row.file as string];
  row["Δ gzip"] = committed === undefined ? "new" : (row.gzip as number) - committed.gzip;
}
console.log(
  "Bytes each test app ships per page (metadata = markers, data-rz attributes, and the state block, as a share of the brotli HTML)",
);
console.table(rows);
recordSnapshot(results, sizes, { check: process.argv.includes("--check") });
