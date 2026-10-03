import { createHash } from "node:crypto";
import { EventEmitter } from "node:events";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { ResolvedConfig } from "vite";
import { afterEach, beforeEach, expect, test, vi, type Mock } from "vitest";

import reze, { DEFAULT_ROUTE_EXTENSIONS, type Options } from "../src";

const compile = vi.hoisted(() => vi.fn());
vi.mock("@rezejs/compiler", () => ({ compile }));

type Severity = "error" | "warn" | "info";

interface EnvironmentConfig {
  consumer: "client" | "server";
  dev: { sourcemap: boolean | { js?: boolean; css?: boolean } };
  build: { sourcemap: boolean | "inline" | "hidden" };
}

const buildConfig = {
  command: "build",
  isProduction: true,
  server: { hmr: undefined },
} as unknown as ResolvedConfig;
const clientEnvironment: EnvironmentConfig = {
  consumer: "client",
  dev: { sourcemap: { js: true } },
  build: { sourcemap: false },
};

const serveConfig = {
  command: "serve",
  isProduction: false,
  server: { hmr: undefined },
} as unknown as ResolvedConfig;

function diagnostic(code: string, severity: Severity) {
  return {
    code,
    severity,
    message: `[${code}] message`,
    file: "/src/App.tsx",
    start: { offset: 10, line: 2, column: 4 },
    end: { offset: 14, line: 2, column: 8 },
    path: ["<App>", "div"],
    labels: [],
    fixes: [],
    data: {},
    docs: `https://example.test/SKILL.md#${code.toLowerCase()}`,
    rendered: `[${code}] message\n  at /src/App.tsx:2:5\n`,
  };
}

interface Harness {
  warn: Mock;
  transform(code: string, id: string): unknown;
}

function setup(config: ResolvedConfig, environment: EnvironmentConfig, options?: Omit<Options, "fileRoutes">): Harness {
  const plugin = reze(options);
  (plugin.configResolved as (config: ResolvedConfig) => void)(config);
  const warn = vi.fn();
  const context = { environment: { config: environment }, warn };
  const hook = plugin.transform as { handler: (code: string, id: string) => unknown };
  return { warn, transform: (code, id) => hook.handler.call(context, code, id) };
}

let dir: string;

beforeEach(() => {
  compile.mockReset();
  compile.mockReturnValue({ code: "out", map: undefined, diagnostics: [] });
  dir = mkdtempSync(join(tmpdir(), "reze-vite-plugin-"));
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

test("a production build compiles without source maps, debug names or hot-swap", () => {
  const { transform } = setup(buildConfig, clientEnvironment);
  expect(transform("src", "/src/App.tsx")).toEqual({ code: "out", map: null });
  expect(compile).toHaveBeenCalledWith("src", "/src/App.tsx", {
    sourceMap: false,
    debugNames: false,
    hot: false,
  });
});

test("the dev server compiles with source maps, debug names and hot-swap", () => {
  const { transform } = setup(serveConfig, clientEnvironment);
  transform("src", "/src/App.tsx");
  expect(compile).toHaveBeenCalledWith("src", "/src/App.tsx", {
    sourceMap: true,
    debugNames: true,
    hot: true,
  });
});

test("a build with sourcemap enabled asks for a source map", () => {
  const { transform } = setup(buildConfig, { ...clientEnvironment, build: { sourcemap: "hidden" } });
  transform("src", "/src/App.tsx");
  expect(compile).toHaveBeenCalledWith("src", "/src/App.tsx", expect.objectContaining({ sourceMap: true }));
});

test("dev sourcemap without js skips the source map", () => {
  const { transform } = setup(serveConfig, { ...clientEnvironment, dev: { sourcemap: { js: false } } });
  transform("src", "/src/App.tsx");
  expect(compile).toHaveBeenCalledWith("src", "/src/App.tsx", expect.objectContaining({ sourceMap: false }));
});

test("disabling server.hmr turns hot-swap off", () => {
  const config = { ...serveConfig, server: { hmr: false } } as unknown as ResolvedConfig;
  const { transform } = setup(config, clientEnvironment);
  transform("src", "/src/App.tsx");
  expect(compile).toHaveBeenCalledWith("src", "/src/App.tsx", expect.objectContaining({ hot: false }));
});

test("only client environments are transformed", () => {
  const plugin = reze();
  const applies = plugin.applyToEnvironment as (environment: { config: EnvironmentConfig }) => boolean;
  expect(applies({ config: { ...clientEnvironment, consumer: "server" } })).toBe(false);
  expect(applies({ config: clientEnvironment })).toBe(true);
});

test("the transform filter selects script modules outside node_modules", () => {
  const hook = reze().transform as { filter: { id: { include: RegExp; exclude: RegExp } } };
  const { include, exclude } = hook.filter.id;
  const selected = (id: string) => include.test(id) && !exclude.test(id);
  expect(selected("/src/App.tsx")).toBe(true);
  expect(selected("/src/util.mjs")).toBe(true);
  expect(selected("/src/App.tsx?v=1")).toBe(true);
  expect(selected("/src/styles.css")).toBe(false);
  expect(selected("/node_modules/lib/index.js")).toBe(false);
});

test("extensions replace the transform filter, spreading the defaults extends it", () => {
  const selectedWith = (extras: string[]) => {
    const hook = reze({ extensions: extras }).transform as { filter: { id: { include: RegExp; exclude: RegExp } } };
    const { include, exclude } = hook.filter.id;
    return (id: string) => include.test(id) && !exclude.test(id);
  };
  const replaced = selectedWith(["mdx", ".md"]);
  expect(replaced("/src/page.mdx")).toBe(true);
  expect(replaced("/src/guide.md")).toBe(true);
  expect(replaced("/src/App.tsx")).toBe(false);
  const extended = selectedWith([...DEFAULT_ROUTE_EXTENSIONS, ".mdx"]);
  expect(extended("/src/page.mdx")).toBe(true);
  expect(extended("/src/App.tsx")).toBe(true);
  expect(extended("/src/util.mjs")).toBe(true);
  expect(extended("/src/styles.css")).toBe(false);
});

test("the query and hash are stripped from the filename", () => {
  const { transform } = setup(buildConfig, clientEnvironment);
  transform("src", "/src/App.tsx?v=1");
  transform("src", "/src/Other.tsx#frag");
  expect(compile.mock.calls.map((call) => call[1])).toEqual(["/src/App.tsx", "/src/Other.tsx"]);
});

test("a module without JSX is left untouched", () => {
  compile.mockReturnValue(null);
  const { transform } = setup(buildConfig, clientEnvironment);
  expect(transform("src", "/src/util.ts")).toBeNull();
});

test("the repair footer follows only the first warning of each code", () => {
  const first = diagnostic("UNKNOWN_ATTRIBUTE", "warn");
  const other = diagnostic("KEY_ON_ELEMENT", "warn");
  compile.mockReturnValue({ code: "out", map: "{}", diagnostics: [first, first, other] });
  const { transform, warn } = setup(buildConfig, clientEnvironment);
  expect(transform("src", "/src/App.tsx")).toEqual({ code: "out", map: "{}" });
  transform("src", "/src/App.tsx");

  const messages = warn.mock.calls.map(([log]) => log.message as string);
  expect(messages).toEqual([
    `[UNKNOWN_ATTRIBUTE] message
  at /src/App.tsx:2:5
  repair guide: node_modules/@rezejs/compiler/skills/reze-compiler-diagnostics/SKILL.md#unknown_attribute
                https://example.test/SKILL.md#unknown_attribute`,
    first.rendered,
    `[KEY_ON_ELEMENT] message
  at /src/App.tsx:2:5
  repair guide: node_modules/@rezejs/compiler/skills/reze-compiler-diagnostics/SKILL.md#key_on_element
                https://example.test/SKILL.md#key_on_element`,
    first.rendered,
    first.rendered,
    other.rendered,
  ]);
  expect(warn.mock.calls[0]![0]).toMatchObject({
    id: "/src/App.tsx",
    loc: { file: "/src/App.tsx", line: 2, column: 4 },
  });
});

test("info diagnostics are never printed", () => {
  compile.mockReturnValue({ code: "out", diagnostics: [diagnostic("SIGNAL_FOLDED", "info")] });
  const { transform, warn } = setup(buildConfig, clientEnvironment);
  transform("src", "/src/App.tsx");
  expect(warn).not.toHaveBeenCalled();
});

test("compile errors throw with the overlay fields", () => {
  const error = diagnostic("CONTROL_FLOW_AS_VALUE", "error");
  const second = { ...diagnostic("MATCH_OUTSIDE_SWITCH", "error"), start: { offset: 30, line: 4, column: 1 } };
  compile.mockReturnValue({ diagnostics: [error, second] });
  const { transform } = setup(buildConfig, clientEnvironment);

  let thrown: unknown;
  try {
    transform("src", "/src/App.tsx?v=1");
  } catch (caught) {
    thrown = caught;
  }
  expect(thrown).toBeInstanceOf(Error);
  expect(thrown).toMatchObject({
    id: "/src/App.tsx?v=1",
    loc: { file: "/src/App.tsx", line: 2, column: 4 },
    frame: error.rendered,
    plugin: "reze-js",
    diagnostics: [error, second],
  });
  const message = (thrown as Error).message;
  expect(message).toContain("[CONTROL_FLOW_AS_VALUE] message");
  expect(message).toContain("[MATCH_OUTSIDE_SWITCH] message");
  expect(message).toContain("#control_flow_as_value");
});

test("every diagnostic, info included, is appended to the JSONL sink", () => {
  const jsonl = join(dir, "nested", "diagnostics.jsonl");
  const warning = diagnostic("UNKNOWN_ATTRIBUTE", "warn");
  const info = diagnostic("SIGNAL_FOLDED", "info");
  const error = diagnostic("CONTROL_FLOW_AS_VALUE", "error");
  const { transform } = setup(buildConfig, clientEnvironment, { diagnostics: { jsonl } });

  compile.mockReturnValue({ code: "out", diagnostics: [warning, info] });
  transform("src", "/src/App.tsx");
  compile.mockReturnValue({ diagnostics: [error] });
  expect(() => transform("src", "/src/Bad.tsx")).toThrow();

  const lines = readFileSync(jsonl, "utf8").split("\n");
  expect(lines.pop()).toBe("");
  expect(lines.map((line) => JSON.parse(line))).toEqual([warning, info, error]);
});

const profileComponents = [{ component: "App", file: "/src/App.tsx", mounts: 2, props: 1, reruns: 0, writes: 5 }];

function writeProfileRecord(profileDir: string, file: string, hash: string): void {
  mkdirSync(profileDir, { recursive: true });
  const key = createHash("sha256").update(file).digest("hex").slice(0, 32);
  writeFileSync(join(profileDir, `${key}.json`), JSON.stringify({ v: 1, file, hash, components: profileComponents }));
}

function profileTransform(config: ResolvedConfig, profileDir: string) {
  const plugin = reze({ profile: { dir: profileDir } });
  (plugin.configResolved as (config: ResolvedConfig) => void)(config);
  const hook = plugin.transform as { handler: (code: string, id: string) => unknown };
  const context = { environment: { config: clientEnvironment }, warn: vi.fn() };
  return (code: string, id: string) => hook.handler.call(context, code, id);
}

test("profile facts for the current source are passed to compile", () => {
  const profileDir = join(dir, "profiles");
  writeProfileRecord(profileDir, "/src/App.tsx", "825994195cfb21c9");
  const transform = profileTransform({ ...buildConfig, root: dir } as ResolvedConfig, profileDir);
  transform("src", "/src/App.tsx");
  expect(compile).toHaveBeenCalledWith("src", "/src/App.tsx", {
    sourceMap: false,
    debugNames: false,
    hot: false,
    profile: { hash: "825994195cfb21c9", components: profileComponents },
  });
});

test("stale profile facts are not passed to compile", () => {
  const profileDir = join(dir, "profiles");
  writeProfileRecord(profileDir, "/src/App.tsx", "0000000000000000");
  const transform = profileTransform({ ...buildConfig, root: dir } as ResolvedConfig, profileDir);
  transform("src", "/src/App.tsx");
  expect(compile).toHaveBeenCalledWith("src", "/src/App.tsx", {
    sourceMap: false,
    debugNames: false,
    hot: false,
  });
});

function profileEndpoint(profileDir: string) {
  const plugin = reze({ profile: { dir: profileDir } });
  (plugin.configResolved as (config: ResolvedConfig) => void)({ ...serveConfig, root: dir } as ResolvedConfig);
  const use = vi.fn();
  (plugin.configureServer as unknown as (server: { middlewares: { use: Mock } }) => void)({
    middlewares: { use },
  });
  expect(use).toHaveBeenCalledTimes(1);
  expect(use.mock.calls[0]![0]).toBe("/__reze/profile");
  const hook = plugin.transform as { handler: (code: string, id: string) => unknown };
  const context = { environment: { config: clientEnvironment }, warn: vi.fn() };
  return {
    transform: (code: string, id: string) => hook.handler.call(context, code, id),
    post: (body: unknown) => {
      const handler = use.mock.calls[0]![1] as (req: unknown, res: unknown, next: () => void) => void;
      const req = Object.assign(new EventEmitter(), { method: "POST" });
      const res = { statusCode: 0, end: vi.fn() };
      handler(req, res, vi.fn());
      req.emit("data", typeof body === "string" ? body : JSON.stringify(body));
      req.emit("end");
      return res;
    },
  };
}

test("posted session trees accumulate per-file facts", () => {
  const profileDir = join(dir, "profiles");
  const { transform, post } = profileEndpoint(profileDir);
  transform("src", "/src/App.tsx");
  const tree = {
    v: 1,
    components: [{ component: "App", file: "/src/App.tsx", mounts: 1, props: 2, reruns: 0, writes: 3 }],
  };
  expect(post(tree).statusCode).toBe(200);
  expect(post(tree).statusCode).toBe(200);
  const names = readdirSync(profileDir);
  expect(names).toHaveLength(1);
  expect(JSON.parse(readFileSync(join(profileDir, names[0]!), "utf8"))).toEqual({
    v: 1,
    file: "/src/App.tsx",
    hash: "825994195cfb21c9",
    components: [{ component: "App", file: "/src/App.tsx", mounts: 2, props: 2, reruns: 0, writes: 6 }],
  });
});

test("the profile endpoint ignores unknown files and invalid trees", () => {
  const profileDir = join(dir, "profiles");
  const { post } = profileEndpoint(profileDir);
  const unknown = {
    v: 1,
    components: [{ component: "Never", file: "/src/Never.tsx", mounts: 1, props: 0, reruns: 0, writes: 0 }],
  };
  expect(post(unknown).statusCode).toBe(200);
  expect(post("{oops").statusCode).toBe(400);
  expect(existsSync(profileDir)).toBe(false);
});
