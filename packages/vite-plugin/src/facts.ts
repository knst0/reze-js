import { readFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";

import { analyze } from "@rezejs/compiler";

import { profileHash } from "./hash";

export type Resolver = (specifier: string, importer: string) => Promise<string | undefined>;

interface ReexportFacts {
  role: "reexport";
  from: string;
  name: string;
}

interface OtherExportFacts {
  role: string;
  [key: string]: unknown;
}

type ExportFacts = ReexportFacts | OtherExportFacts;

export interface ModuleFacts {
  v: number;
  compiler: string;
  hash: string;
  exports: Record<string, ExportFacts>;
}

interface PackageJson {
  name?: string;
  exports?: unknown;
  reze?: { manifest?: unknown };
}

interface Package {
  dir: string;
  json: PackageJson;
}

interface OwnFacts {
  hash: string;
  facts: ModuleFacts | undefined;
}

const StaticImport = /(?:^|[\s;])import\s+(?:type\s+)?(?:[^'"`;]*?\sfrom\s+)?["']([^"'\n]+)["']/gm;

export function staticImportSpecifiers(source: string): string[] {
  const specifiers = new Set<string>();
  for (const match of source.matchAll(StaticImport)) specifiers.add(match[1]!);
  return [...specifiers];
}

function readText(file: string): string | undefined {
  try {
    return readFileSync(file, "utf8");
  } catch {
    return undefined;
  }
}

function readJson(file: string): unknown {
  const text = readText(file);
  if (text === undefined) return undefined;
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

function isReexport(entry: ExportFacts): entry is ReexportFacts {
  return entry.role === "reexport" && typeof entry.from === "string" && typeof entry.name === "string";
}

function isModuleFacts(value: unknown): value is ModuleFacts {
  return typeof value === "object" && value !== null && typeof (value as ModuleFacts).hash === "string";
}

function declaresCondition(value: unknown, condition: string): boolean {
  if (typeof value !== "object" || value === null) return false;
  if (Array.isArray(value)) return value.some((item) => declaresCondition(item, condition));
  return condition in value || Object.values(value).some((item) => declaresCondition(item, condition));
}

/**
 * Module facts for the plugin's application modules and for libraries. Facts of a module depend only on its own
 * source, so `analyze` results are cached by content hash and never need a fixed point across imports.
 */
export class ModuleFactsStore {
  private readonly own = new Map<string, OwnFacts>();
  private readonly packageJsons = new Map<string, PackageJson | null>();
  private readonly manifests = new Map<string, { text: string; entries: unknown }>();
  private readonly consumed = new Map<string, Map<string, string>>();
  private readonly staleWarned = new Set<string>();
  private appRoot: string | undefined;
  private warn: ((message: string) => void) | undefined;

  constructor(
    private readonly analyzed: readonly string[],
    private readonly isSource: (file: string) => boolean,
  ) {}

  configure(settings: { root: string; warn?: (message: string) => void }): void {
    this.appRoot = this.packageOf(join(settings.root, "index.html"))?.dir;
    this.warn = settings.warn;
  }

  declaresReze(file: string): boolean {
    const owner = this.packageOf(file);
    return owner !== undefined && declaresCondition(owner.json.exports, "reze");
  }

  recordCompiled(file: string, source: string, facts: ModuleFacts | undefined): void {
    this.own.set(file, { hash: profileHash(source), facts });
  }

  async factsFor(file: string, specifiers: readonly string[], resolve: Resolver): Promise<Record<string, ModuleFacts>> {
    const used = new Map<string, string>();
    this.consumed.set(file, used);
    const facts: Record<string, ModuleFacts> = {};
    for (const specifier of specifiers) {
      const target = await resolve(specifier, file);
      if (target === undefined) continue;
      const resolved = await this.moduleFacts(target, resolve, used, new Set());
      if (resolved !== undefined) facts[specifier] = resolved;
    }
    return facts;
  }

  /** Importers that consumed a stale version of `file`'s facts; `file` is re-read from disk, so call this before its re-transform. */
  async importersToInvalidate(file: string, resolve: Resolver): Promise<string[]> {
    const importers = [...this.consumed].filter(([, used]) => used.has(file)).map(([importer]) => importer);
    if (importers.length === 0) return [];
    const current = JSON.stringify((await this.moduleFacts(file, resolve, new Map(), new Set()))?.exports ?? null);
    return importers.filter((importer) => this.consumed.get(importer)?.get(file) !== current);
  }

  private async moduleFacts(
    file: string,
    resolve: Resolver,
    used: Map<string, string>,
    visiting: ReadonlySet<string>,
  ): Promise<ModuleFacts | undefined> {
    if (visiting.has(file)) return undefined;
    const base = this.baseFacts(file);
    const facts = base === undefined ? undefined : await this.flatten(base, file, resolve, used, new Set([...visiting, file]));
    used.set(file, JSON.stringify(facts?.exports ?? null));
    return facts;
  }

  private async flatten(
    base: ModuleFacts,
    file: string,
    resolve: Resolver,
    used: Map<string, string>,
    visiting: ReadonlySet<string>,
  ): Promise<ModuleFacts> {
    const exports: Record<string, ExportFacts> = {};
    for (const [name, entry] of Object.entries(base.exports)) {
      if (!isReexport(entry)) {
        exports[name] = entry;
        continue;
      }
      const target = await resolve(entry.from, file);
      const exported = target === undefined ? undefined : (await this.moduleFacts(target, resolve, used, visiting))?.exports[entry.name];
      if (exported !== undefined) exports[name] = exported;
    }
    return { ...base, exports };
  }

  private baseFacts(file: string): ModuleFacts | undefined {
    const owner = this.packageOf(file);
    if (owner === undefined) return /[\\/]node_modules[\\/]/.test(file) ? undefined : this.ownFacts(file);
    if (owner.dir === this.appRoot) return this.ownFacts(file);
    const source = readText(file);
    if (source === undefined) return undefined;
    const entry = this.manifestOf(owner)?.[relative(owner.dir, file).split(sep).join("/")];
    if (isModuleFacts(entry) && entry.hash === profileHash(source)) return entry;
    const name = owner.json.name;
    if (
      name !== undefined &&
      this.analyzed.some((pattern) => (pattern.endsWith("/*") ? name.startsWith(pattern.slice(0, -1)) : name === pattern))
    ) {
      return this.ownFacts(file);
    }
    if (entry !== undefined) this.warnStale(name ?? owner.dir);
    return undefined;
  }

  private ownFacts(file: string): ModuleFacts | undefined {
    if (!this.isSource(file)) return undefined;
    const source = readText(file);
    if (source === undefined) return undefined;
    const hash = profileHash(source);
    const cached = this.own.get(file);
    if (cached !== undefined && cached.hash === hash) return cached.facts;
    const facts = analyze(source, file).facts as ModuleFacts | undefined;
    this.own.set(file, { hash, facts });
    return facts;
  }

  private packageOf(file: string): Package | undefined {
    for (let dir = dirname(file); ; dir = dirname(dir)) {
      const json = this.packageJson(dir);
      if (json !== undefined) return { dir, json };
      if (dirname(dir) === dir) return undefined;
    }
  }

  private packageJson(dir: string): PackageJson | undefined {
    let json = this.packageJsons.get(dir);
    if (json === undefined) {
      json = (readJson(join(dir, "package.json")) as PackageJson | undefined) ?? null;
      this.packageJsons.set(dir, json);
    }
    return json ?? undefined;
  }

  private manifestOf(owner: Package): Record<string, unknown> | undefined {
    const manifest = owner.json.reze?.manifest;
    if (typeof manifest !== "string") return undefined;
    const path = join(owner.dir, manifest);
    const text = readText(path);
    if (text === undefined) return undefined;
    const cached = this.manifests.get(path);
    if (cached !== undefined && cached.text === text) return cached.entries as Record<string, unknown> | undefined;
    const entries = readJson(path);
    this.manifests.set(path, { text, entries });
    return entries as Record<string, unknown> | undefined;
  }

  private warnStale(name: string): void {
    if (this.warn === undefined || this.staleWarned.has(name)) return;
    this.staleWarned.add(name);
    this.warn(
      `reze: manifest of "${name}" is stale. Regenerate it with \`reze-manifest\` in that package, or add "${name}" to reze({ analyze }).`,
    );
  }
}
