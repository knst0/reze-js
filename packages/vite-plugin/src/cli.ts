#!/usr/bin/env node
import { existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const usage = `Usage: reze check --render-diff --rust <origin> [--build <dir>] [path...]

Renders each path through the JS render of the build in <dir> (default: dist), whose loaders run
in the Rust server at <origin>, and through the Rust render of that server, and compares the
status and the bytes. Without paths, renders every static route and each \`entries\` item.`;

async function entry(dir: string, name: string): Promise<Record<string, unknown>> {
  const file = [".js", ".mjs"].map((ext) => join(dir, "server", name + ext)).find(existsSync);
  if (!file) throw new Error(`${join(dir, "server", name)}.js is missing: build the app with \`vite build\` first.`);
  return import(pathToFileURL(file).href);
}

/** The first line where `a` and `b` differ, with its number. */
function firstDifference(a: string, b: string): [number, string, string] {
  const left = a.split("\n");
  const right = b.split("\n");
  const line = left.findIndex((text, index) => text !== right[index]);
  const at = line === -1 ? left.length : line;
  return [at + 1, left[at] ?? "(end)", right[at] ?? "(end)"];
}

async function renderDiff(rust: string, build: string, paths: string[]): Promise<number> {
  const server = await entry(build, "server");
  if (typeof server.createHandler !== "function") throw new Error("The render diff needs the default server entry, not src/server.ts.");
  const handler = (server.createHandler as (options: object) => (request: Request) => Promise<Response>)({ loaders: rust });
  if (paths.length === 0) paths = await ((await entry(build, "prerender")).every as () => Promise<string[]>)();
  const headers = { "rz-prerender": "1" };
  let failed = 0;
  for (const path of paths) {
    const [js, native] = await Promise.all([
      handler(new Request(new URL(path, "http://localhost"), { headers })),
      fetch(new URL(path, rust), { headers, redirect: "manual" }),
    ]);
    const [jsBody, rustBody] = await Promise.all([js.text(), native.text()]);
    if (js.status !== native.status) {
      failed++;
      console.error(`✗ ${path}: status ${js.status} on JS, ${native.status} on Rust`);
    } else if (jsBody !== rustBody) {
      failed++;
      const [line, left, right] = firstDifference(jsBody, rustBody);
      console.error(`✗ ${path}: the bodies differ at line ${line}\n  JS:   ${left}\n  Rust: ${right}`);
    } else {
      console.log(`✓ ${path}`);
    }
  }
  console.log(`${paths.length - failed} of ${paths.length} paths render the same.`);
  return failed === 0 ? 0 : 1;
}

async function main(argv: string[]): Promise<number> {
  const { values, positionals } = parseArgs({
    args: argv,
    allowPositionals: true,
    options: {
      "render-diff": { type: "boolean" },
      rust: { type: "string" },
      build: { type: "string", default: "dist" },
      help: { type: "boolean", short: "h" },
    },
  });
  const [command, ...paths] = positionals;
  if (values.help || command !== "check" || !values["render-diff"] || !values.rust) {
    console.log(usage);
    return values.help ? 0 : 2;
  }
  return renderDiff(values.rust, resolve(values.build), paths);
}

main(process.argv.slice(2)).then(
  (code) => (process.exitCode = code),
  (error: unknown) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  },
);
