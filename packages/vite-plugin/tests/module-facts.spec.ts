import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ModuleFactsStore, staticImportSpecifiers } from "../src/facts";
import { profileHash } from "../src/hash";
import { writeManifest } from "../src/manifest";

const LIB_SOURCE = `import { signal } from "reze-js";\nlet n = signal(0);\nexport function read() { return n; }\n`;
const READ_FACTS = { role: "function", reads: "fixed", returns: "number" };

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "reze-module-facts-"));
  write("package.json", JSON.stringify({ name: "app" }));
  write("index.html", "");
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

function write(file: string, text: string): string {
  const path = join(root, file);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
  return path;
}

async function resolve(specifier: string, importer: string): Promise<string | undefined> {
  if (specifier.startsWith(".")) {
    const path = join(dirname(importer), `${specifier}.ts`);
    return existsSync(path) ? path : undefined;
  }
  const path = join(root, "node_modules", specifier, "src/lib.ts");
  return existsSync(path) ? path : undefined;
}

function storeFor(analyzed: string[] = [], warn?: (message: string) => void): ModuleFactsStore {
  const store = new ModuleFactsStore(analyzed, () => true);
  store.configure({ root, warn });
  return store;
}

function writeLibrary(source = LIB_SOURCE): string {
  write(
    "node_modules/@acme/ui/package.json",
    JSON.stringify({
      name: "@acme/ui",
      exports: { ".": { reze: "./src/lib.ts", default: "./dist/lib.js" } },
      reze: { manifest: "reze.json" },
    }),
  );
  return write("node_modules/@acme/ui/src/lib.ts", source);
}

const libraryDir = () => join(root, "node_modules/@acme/ui");
const mainPath = () => join(root, "src/main.ts");

describe("staticImportSpecifiers", () => {
  it("lists static imports, including multi-line ones, and skips dynamic import()", () => {
    const source = `import a from "./a";\nimport {\n  b,\n} from "./b";\nimport "./side-effect";\nconst late = await import("./lazy");`;
    expect(staticImportSpecifiers(source)).toEqual(["./a", "./b", "./side-effect"]);
  });
});

describe("application modules", () => {
  it("gives a re-export the target's facts under the re-exported name", async () => {
    write("src/b.ts", LIB_SOURCE);
    write("src/a.ts", `export { read as reading } from "./b";`);
    const facts = await storeFor().factsFor(mainPath(), ["./a"], resolve);
    expect(facts["./a"].exports).toEqual({ reading: READ_FACTS });
  });

  it("drops a re-export whose target does not export the name", async () => {
    write("src/b.ts", LIB_SOURCE);
    write("src/a.ts", `export { missing } from "./b";`);
    const facts = await storeFor().factsFor(mainPath(), ["./a"], resolve);
    expect(facts["./a"].exports).toEqual({});
  });

  it("terminates on cyclic re-exports", async () => {
    write("src/a.ts", `export { read } from "./b";`);
    write("src/b.ts", `export { read } from "./a";`);
    const facts = await storeFor().factsFor(mainPath(), ["./a"], resolve);
    expect(facts["./a"].exports).toEqual({});
  });

  it("omits facts of a module the compiler refuses", async () => {
    write("src/b.ts", `import { signal } from "reze-js";\nlet n = signal(0);\nexport default n;\nexport const bump = () => (n += 1);\n`);
    const facts = await storeFor().factsFor(mainPath(), ["./b"], resolve);
    expect(facts).toEqual({});
  });

  it("invalidates an importer only when an export it consumes changes", async () => {
    const dependency = write("src/a.ts", LIB_SOURCE);
    const store = storeFor();
    await store.factsFor(mainPath(), ["./a"], resolve);

    write("src/a.ts", `${LIB_SOURCE}\n`);
    expect(await store.importersToInvalidate(dependency, resolve)).toEqual([]);

    write("src/a.ts", `export function read() { return "x"; }\n`);
    expect(await store.importersToInvalidate(dependency, resolve)).toEqual([mainPath()]);
  });

  it("keeps reporting a stale importer to every caller until it consumes the change", async () => {
    const dependency = write("src/a.ts", LIB_SOURCE);
    const store = storeFor();
    await store.factsFor(mainPath(), ["./a"], resolve);

    write("src/a.ts", `export function read() { return "x"; }\n`);
    expect(await store.importersToInvalidate(dependency, resolve)).toEqual([mainPath()]);
    expect(await store.importersToInvalidate(dependency, resolve)).toEqual([mainPath()]);

    await store.factsFor(mainPath(), ["./a"], resolve);
    expect(await store.importersToInvalidate(dependency, resolve)).toEqual([]);
  });
});

describe("library modules", () => {
  it("uses a manifest entry whose hash matches the source", async () => {
    writeLibrary();
    writeManifest(libraryDir());
    const facts = await storeFor().factsFor(mainPath(), ["@acme/ui"], resolve);
    expect(facts["@acme/ui"].exports).toEqual({ read: READ_FACTS });
  });

  it("treats a library without a manifest as opaque", async () => {
    writeLibrary();
    const facts = await storeFor().factsFor(mainPath(), ["@acme/ui"], resolve);
    expect(facts).toEqual({});
  });

  it("ignores a stale manifest entry and warns once per package", async () => {
    writeLibrary();
    writeManifest(libraryDir());
    writeLibrary(`${LIB_SOURCE}\n`);
    const warn = vi.fn();
    const store = storeFor([], warn);

    expect(await store.factsFor(mainPath(), ["@acme/ui"], resolve)).toEqual({});
    await store.factsFor(mainPath(), ["@acme/ui"], resolve);

    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0][0]).toContain("@acme/ui");
  });

  it("analyzes a listed package whose manifest is stale", async () => {
    writeLibrary();
    writeManifest(libraryDir());
    writeLibrary(`${LIB_SOURCE}\n`);
    const facts = await storeFor(["@acme/ui"]).factsFor(mainPath(), ["@acme/ui"], resolve);
    expect(facts["@acme/ui"].exports).toEqual({ read: READ_FACTS });
  });

  it("prefers a valid manifest entry over analysis when the package is listed", async () => {
    writeLibrary();
    const manifestPath = writeManifest(libraryDir()).path;
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    manifest["src/lib.ts"].exports.read = { role: "value" };
    writeFileSync(manifestPath, JSON.stringify(manifest));

    const facts = await storeFor(["@acme/ui"]).factsFor(mainPath(), ["@acme/ui"], resolve);
    expect(facts["@acme/ui"].exports).toEqual({ read: { role: "value" } });
  });

  it("writes manifest hashes that match the plugin's hash of the source", () => {
    writeLibrary();
    const manifest = JSON.parse(readFileSync(writeManifest(libraryDir()).path, "utf8"));
    expect(manifest["src/lib.ts"].hash).toBe(profileHash(LIB_SOURCE));
  });

  it("reports modules the compiler refuses instead of writing them", () => {
    writeLibrary();
    write(
      "node_modules/@acme/ui/src/bad.ts",
      `import { signal } from "reze-js";\nlet m = signal(1);\nexport default m;\nexport const bump = () => (m += 1);\n`,
    );
    const report = writeManifest(libraryDir());
    expect(report.written).toBe(1);
    expect(report.skipped).toEqual([{ file: "src/bad.ts", reason: "SIGNAL_DEFAULT_EXPORT" }]);
  });

  it("rejects a package without a reze.manifest", () => {
    write("node_modules/@acme/ui/package.json", JSON.stringify({ name: "@acme/ui", exports: { reze: "./src/lib.ts" } }));
    expect(() => writeManifest(libraryDir())).toThrow("no reze.manifest");
  });

  it("declares reze only for packages with a reze export condition", () => {
    const library = writeLibrary();
    const store = storeFor();
    expect(store.declaresReze(library)).toBe(true);
    expect(store.declaresReze(mainPath())).toBe(false);
  });
});
