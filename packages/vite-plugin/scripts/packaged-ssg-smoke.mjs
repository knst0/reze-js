#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, extname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { chromium, firefox, webkit } from "playwright";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const compiler = join(repo, "packages/compiler");
const fixturesRoot = join(repo, "packages/vite-plugin/tests/fixtures/ssg");
const packages = ["signals", "dom", "router", "reze-js", "vite-plugin"];
const engines = { chromium, firefox, webkit };
const fixtures = {
  "standalone-basics": { pages: ["/"] },
  "router-full": {
    pages: ["/", "/counter/", "/blog/a/"],
    paths: {
      "/blog/:id": [{ id: "a" }, { id: "b" }],
      "/opt/:id?": [{}, { id: "x" }],
      "/files/*rest": [{ rest: ["a", "b"] }],
      "/ghost/:id": [],
    },
  },
  "mdx-post": { pages: ["/docs/guide/"] },
};
const { values: options } = parseArgs({ options: {
  vite: { type: "string", multiple: true },
  browsers: { type: "string", multiple: true },
  fixtures: { type: "string", multiple: true },
  "artifacts-dir": { type: "string" },
  keep: { type: "boolean", default: false },
  help: { type: "boolean", short: "h", default: false },
} });
const list = (values, fallback) => (values ?? fallback).flatMap(value => value.split(","));
const env = { ...process.env };
delete env.NAPI_RS_NATIVE_LIBRARY_PATH;

async function run(command, args, cwd) {
  const result = Promise.withResolvers();
  const child = spawn(command, args, { cwd, env, stdio: ["ignore", "pipe", "pipe"], timeout: 180_000 });
  let output = "";
  child.stdout.on("data", chunk => { output += chunk; });
  child.stderr.on("data", chunk => { output += chunk; });
  child.once("error", result.reject);
  child.once("close", code => {
    if (code === 0) result.resolve(output);
    else result.reject(new Error(`${command} ${args.join(" ")} exited ${code}\n${output.slice(-8000)}`));
  });
  return result.promise;
}

async function json(path) {
  return JSON.parse(await readFile(path, "utf8"));
}

async function stageCompiler(scratch) {
  const stage = join(scratch, "compiler");
  const source = options["artifacts-dir"] === undefined ? compiler : resolve(options["artifacts-dir"]);
  await mkdir(stage);
  for (const entry of ["package.json", "index.js", "index.d.ts", "skills"]) {
    await cp(join(source, entry), join(stage, entry), { recursive: true });
  }
  if (options["artifacts-dir"] !== undefined) {
    await cp(join(source, "npm"), join(stage, "npm"), { recursive: true });
  } else {
    const artifacts = join(stage, "artifacts", "local");
    await mkdir(artifacts, { recursive: true });
    const host = (await run("rustc", ["-vV"], repo)).match(/^host: (.+)$/m)?.[1];
    const manifest = await json(join(stage, "package.json"));
    assert.ok(manifest.napi.targets.includes(host), `unsupported local Rust host ${host}`);
    manifest.napi.targets = [host];
    await writeFile(join(stage, "package.json"), JSON.stringify(manifest));
    const suffix = process.platform === "linux"
      ? `-${process.report.getReport().header.glibcVersionRuntime === undefined ? "musl" : "gnu"}`
      : process.platform === "win32" ? "-msvc" : "";
    const binding = `${manifest.napi.binaryName}.${process.platform}-${process.arch}${suffix}.node`;
    await cp(join(compiler, binding), join(artifacts, binding));
    const napi = join(compiler, "node_modules/.bin/napi");
    await run(napi, ["create-npm-dirs"], stage);
    await run(napi, ["artifacts", "--output-dir", "artifacts"], stage);
    await run(napi, ["pre-publish", "--skip-optional-publish", "--no-gh-release"], stage);
  }
  return stage;
}

async function pack(dir, destination) {
  const manifest = await json(join(dir, "package.json"));
  const tarball = join(destination, `${manifest.name.replaceAll("/", "-").replace("@", "")}.tgz`);
  await run("pnpm", ["pack", "--out", tarball], dir);
  return { manifest, tarball };
}

async function packAll(scratch) {
  const stage = await stageCompiler(scratch);
  const destination = join(scratch, "tarballs");
  await mkdir(destination);
  const packed = [];
  for (const name of packages) packed.push(await pack(join(repo, "packages", name), destination));
  const root = await pack(stage, destination);
  packed.push(root);
  const platforms = [];
  for (const name of await readdir(join(stage, "npm"))) {
    const directory = join(stage, "npm", name);
    assert.ok((await readdir(directory)).some(file => file.endsWith(".node")), `missing native artifact for ${name}`);
    const platform = await pack(directory, destination);
    assert.equal(root.manifest.optionalDependencies[platform.manifest.name], platform.manifest.version);
    platforms.push(platform);
  }
  const libc = process.platform === "linux"
    ? (process.report.getReport().header.glibcVersionRuntime === undefined ? "musl" : "glibc")
    : undefined;
  const host = platforms.filter(({ manifest }) => manifest.os.includes(process.platform)
    && manifest.cpu.includes(process.arch) && (libc === undefined || manifest.libc?.includes(libc)));
  assert.equal(host.length, 1, "staged artifacts must contain exactly one compatible host binding");
  packed.push(host[0]);
  console.log(`ok staged ${platforms.length} native artifact packages; host=${host[0].manifest.name}`);
  return packed;
}

async function installConsumer(scratch, packed, version) {
  const project = join(scratch, `vite-${version}`);
  await mkdir(project);
  const root = await json(join(repo, "package.json"));
  const docs = await json(join(repo, "docs/package.json"));
  const devDependencies = { vite: version, typescript: root.devDependencies.typescript, "@types/node": root.devDependencies["@types/node"] };
  for (const name of ["@mdx-js/rollup", "remark-frontmatter", "remark-mdx-frontmatter"]) {
    devDependencies[name] = docs.devDependencies[name];
  }
  await writeFile(join(project, "package.json"), JSON.stringify({
    name: "reze-packaged-consumer", private: true, type: "module",
    dependencies: Object.fromEntries(packed.map(({ manifest, tarball }) => [manifest.name, `file:${tarball}`])),
    devDependencies,
  }));
  await run("npm", ["install", "--no-audit", "--no-fund"], project);
  console.log(await run("node", ["--input-type=module", "-e", `
    import assert from "node:assert/strict";
    import { readdirSync, realpathSync } from "node:fs";
    import { join, sep } from "node:path";
    import { __napiBindingTarget } from "@rezejs/compiler";
    assert.equal(__napiBindingTarget, "native");
    for (const name of ${JSON.stringify(packed.map(entry => entry.manifest.name))}) {
      assert.ok(realpathSync(new URL(import.meta.resolve(name))).startsWith(join(process.cwd(), "node_modules") + sep));
    }
    assert.equal(readdirSync(new URL(".", import.meta.resolve("@rezejs/compiler"))).some(name => name.endsWith(".node")), false);
    console.log("ok external native binding and package resolution");
  `], project));
  return project;
}

async function checkTypes(project) {
  await writeFile(join(project, "tsconfig.types.json"), JSON.stringify({ compilerOptions: {
    target: "esnext", module: "esnext", moduleResolution: "bundler", strict: true, noEmit: true,
  }, include: ["consumer.ts"] }));
  const source = `import reze, { type SsgOptions } from "@rezejs/vite-plugin";
import { defineRoute, type DataOf } from "@rezejs/router";
import { $signal, hydrate, type JSX } from "reze-js";
import { signal } from "@rezejs/signals";
export const post = defineRoute({
  path: "/blog/:id",
  preload: async ({ params }) => ({ id: params.id, title: "Post" }),
  meta: ({ data }) => ({ title: data.title }),
  redirect: ({ data }) => data.id === "old" ? { to: "/blog/new", replace: true } : undefined,
  component: ({ data }) => data.title,
});
export const data: DataOf<{ route: typeof post }> = { id: "a", title: "Post" };
export const config: SsgOptions = { entry: "src/app.tsx", selector: "#app", paths: { "/blog/:id": [{ id: "a" }] } };
export const plugins = reze({ ssg: config });
export const count = signal(0);
export const dsl = $signal(0);
export const mount: (view: () => JSX.Element, root: Element) => Promise<() => void> = hydrate;
`;
  await writeFile(join(project, "consumer.ts"), source);
  const tsc = join(project, "node_modules/.bin/tsc");
  await run(tsc, ["-p", "tsconfig.types.json", "--pretty", "false"], project);
  await writeFile(join(project, "consumer.ts"), `${source}\nconst invalid: DataOf<{ route: typeof post }> = { id: 42, title: "Post" };\n`);
  await assert.rejects(run(tsc, ["-p", "tsconfig.types.json", "--pretty", "false"], project), /TS2322/);
  await writeFile(join(project, "consumer.ts"), source);
}

async function buildFixture(project, name) {
  for (const entry of ["src", "public", "index.html", "dist"]) await rm(join(project, entry), { recursive: true, force: true });
  await cp(join(fixturesRoot, name), project, { recursive: true, filter: path => !path.endsWith("routes.gen.d.ts") });
  const mdx = name === "mdx-post";
  await writeFile(join(project, "vite.config.mjs"), `import reze, { DEFAULT_ROUTE_EXTENSIONS } from "@rezejs/vite-plugin";
${mdx ? `import mdx from "@mdx-js/rollup";
import frontmatter from "remark-frontmatter";
import mdxFrontmatter from "remark-mdx-frontmatter";` : ""}
export default {
  plugins: [${mdx ? `{ ...mdx({ jsx: true, jsxImportSource: "reze-js", providerImportSource: "/src/mdx", remarkPlugins: [frontmatter, mdxFrontmatter] }), enforce: "pre" },` : ""}
    reze({ ${mdx ? 'fileRoutes: { types: false }, extensions: [...DEFAULT_ROUTE_EXTENSIONS, ".mdx"],' : ""}
      ssg: ${JSON.stringify({ entry: "src/app.tsx", paths: fixtures[name].paths })} })],
  build: { target: "esnext", assetsInlineLimit: 0, modulePreload: { polyfill: false } },
};`);
  await run(join(project, "node_modules/.bin/vite"), ["build"], project);
}

async function serve(dist) {
  const server = createServer(async (request, response) => {
    try {
      const pathname = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
      let file = resolve(dist, `.${pathname}`);
      const path = relative(dist, file);
      if (path === ".." || path.startsWith(`..${sep}`)) throw new Error("outside dist");
      if ((await stat(file)).isDirectory()) file = join(file, "index.html");
      const body = await readFile(file);
      const type = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".json": "application/json" }[extname(file)];
      response.writeHead(200, { "content-type": type ?? "application/octet-stream" });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end("not found");
    }
  });
  const ready = Promise.withResolvers();
  server.once("error", ready.reject);
  server.listen(0, "127.0.0.1", ready.resolve);
  await ready.promise;
  return { origin: `http://127.0.0.1:${server.address().port}`, close: () => new Promise(resolve => server.close(resolve)) };
}

async function checkPage(browser, origin, name, path) {
  const context = await browser.newContext();
  const gate = Promise.withResolvers();
  try {
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", error => errors.push(error));
    await page.route("**/*.js", async route => { await gate.promise; await route.continue(); });
    const response = await page.goto(origin + path, { waitUntil: "commit" });
    assert.equal(response.status(), 200, `static page ${name}${path}`);
    await page.waitForFunction(() => document.readyState !== "loading");
    const initial = await page.evaluate(() => {
      const root = document.getElementById("app");
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT);
      const nodes = [root];
      while (walker.nextNode()) if (walker.currentNode.nodeType === 1 || walker.currentNode.textContent.trim()) nodes.push(walker.currentNode);
      window.__packagedNodes = nodes;
      return { text: root.textContent, title: document.title };
    });
    if (name === "standalone-basics") {
      assert.match(initial.text, /3:6:v3/);
      assert.match(initial.text, /settled quote/);
      await page.fill("#name", "edited before boot");
    } else if (name === "mdx-post") {
      assert.equal(initial.title, "Guide");
      assert.match(initial.text, /Published words with emphasis\./);
    } else if (path === "/") assert.match(initial.text, /home-data/);
    else if (path === "/counter/") assert.match(initial.text, /2:4/);
    else assert.match(initial.text, /Post a/);
    gate.resolve();
    await page.waitForLoadState("networkidle");
    assert.equal(await page.evaluate(() => window.__packagedNodes.every(node => node.isConnected)), true);
    if (name === "standalone-basics") {
      assert.equal(await page.inputValue("#name"), "edited before boot");
      await page.click("#inc");
      await page.waitForFunction(() => document.querySelector("#settled").textContent === "4:8:v4");
    } else if (path === "/counter/") {
      await page.click("#counter-inc");
      await page.waitForFunction(() => document.querySelector("#counter-out").textContent === "3:6");
    } else if (name === "mdx-post") {
      assert.equal(await page.$eval('img[alt="diagram"]', image => image.complete && image.naturalWidth > 0), true);
    }
    assert.deepEqual(errors, []);
  } finally {
    gate.resolve();
    await context.close();
  }
}

async function main() {
  if (options.help) {
    console.log("Pack real native/JS tarballs; install, typecheck, build and hydrate outside the workspace.\n--vite 6.4.0,7.0.0,8.0.0 --browsers chromium,firefox,webkit\n--fixtures standalone-basics,router-full,mdx-post --artifacts-dir <staged compiler> --keep");
    return;
  }
  const versions = list(options.vite, ["6.4.0", "7.0.0", "8.0.0"]);
  const names = list(options.fixtures, Object.keys(fixtures));
  const selected = list(options.browsers, [process.env.REZE_SSG_BROWSERS ?? "chromium"]);
  for (const name of names) assert.ok(Object.hasOwn(fixtures, name), `unknown fixture ${name}`);
  for (const name of selected) assert.ok(Object.hasOwn(engines, name), `unknown browser ${name}`);
  const scratch = await mkdtemp(join(tmpdir(), "reze-packaged-"));
  const browsers = new Map();
  console.log(`scratch=${scratch}`);
  try {
    const packed = await packAll(scratch);
    for (const name of selected) browsers.set(name, await engines[name].launch());
    for (const version of versions) {
      const project = await installConsumer(scratch, packed, version);
      await checkTypes(project);
      console.log(`ok external types vite=${version}`);
      for (const name of names) {
        await buildFixture(project, name);
        const server = await serve(join(project, "dist"));
        try {
          assert.equal((await fetch(server.origin + "/__missing__")).status, 404);
          for (const [engine, browser] of browsers) {
            for (const path of fixtures[name].pages) await checkPage(browser, server.origin, name, path);
            console.log(`ok vite=${version} fixture=${name} browser=${engine}`);
          }
        } finally {
          await server.close();
        }
      }
    }
  } finally {
    for (const browser of browsers.values()) await browser.close();
    if (!options.keep) await rm(scratch, { recursive: true, force: true });
  }
}

main().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
