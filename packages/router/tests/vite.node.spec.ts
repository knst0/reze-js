import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { ResolvedConfig } from "vite";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import fileRoutes, { type Options } from "../src/vite";

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "reze-router-"));
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

interface Hooks {
  configResolved(config: ResolvedConfig): void;
  buildStart(): void;
  resolveId(id: string): string | undefined;
  load(id: string): string | undefined;
}

function rezePlugin() {
  return { name: "reze-js", api: { claimLinks: vi.fn() } };
}

function start(options?: Options, plugins: unknown[] = [rezePlugin()], base = "/"): Hooks {
  const hooks = fileRoutes(options) as unknown as Hooks;
  hooks.configResolved({ root, base, plugins } as unknown as ResolvedConfig);
  return hooks;
}

function registeredBase(options: Options | undefined, base: string): string | undefined {
  mkdirSync(join(root, "src", "routes"), { recursive: true });
  writeFileSync(join(root, "src", "routes", "index.tsx"), "");
  start(options, undefined, base).buildStart();
  return /base: (".*");/.exec(readFileSync(join(root, "src", "routes.gen.d.ts"), "utf8"))?.[1];
}

test.each([
  ["/app/", '"/app"'],
  ["./", '""'],
  ["https://cdn.test/assets/", '""'],
])("Vite base %j registers the href base the browser history uses", (base, registered) => {
  expect(registeredBase(undefined, base)).toBe(registered);
});

test('history: "hash" registers "#" as the href base whatever Vite\'s base', () => {
  expect(registeredBase({ history: "hash" }, "/app/")).toBe('"#"');
});

test("claims native anchors for the router through the reze plugin", () => {
  const reze = rezePlugin();
  start(undefined, [{ name: "other" }, reze]);
  expect(reze.api.claimLinks).toHaveBeenCalledExactlyOnceWith("@rezejs/router");
});

test("links: false leaves anchors unclaimed", () => {
  const reze = rezePlugin();
  start({ links: false }, [reze]);
  expect(reze.api.claimLinks).not.toHaveBeenCalled();
});

test("a missing reze plugin throws", () => {
  expect(() => start(undefined, [])).toThrow("[reze-router] @rezejs/vite-plugin not found");
});

test("serves the scanned routes as a virtual module and writes the declaration file", () => {
  const dir = join(root, "src", "routes");
  mkdirSync(join(dir, "blog"), { recursive: true });
  writeFileSync(join(dir, "index.tsx"), "");
  writeFileSync(join(dir, "blog", "[id].tsx"), "");
  const hooks = start();
  hooks.buildStart();
  const id = hooks.resolveId("virtual:reze-routes")!;
  const posixDir = dir.replaceAll("\\", "/");
  expect(hooks.load(id)).toContain(
    `export const routes = [{ path: "/blog/:id", load: () => import(${JSON.stringify(posixDir + "/blog/[id].tsx")}) }, ` +
      `{ path: "/", load: () => import(${JSON.stringify(posixDir + "/index.tsx")}) }];`,
  );
  expect(hooks.load(id)).toContain('export const paths = { "byId": (value, search, hash) => href(`/blog/${enc(value)}`, search, hash)');
  expect(hooks.resolveId("other")).toBeUndefined();
});
test("a missing routes directory throws", () => {
  expect(() => start().buildStart()).toThrow(`[reze-router] routes directory not found: ${join(root, "src", "routes")}`);
});
