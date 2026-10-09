export type AppMode = { kind: "router"; hasShell: boolean } | { kind: "standalone" };

interface ModuleExports {
  explicit: Record<string, string>;
  stars: string[];
}

const ExportList = /export\s*\{([^}]*)\}\s*(?:from\s*("|')([^"']+)\2\s*)?;/g;
const ExportStar = /export\s*\*\s*from\s*("|')([^"']+)\1\s*;/g;
const ExportConst = /export\s*(?:const|let|var|function|class|async\s+function)\s+([A-Za-z_$][\w$]*)/g;
const ExportDefault = /export\s+default\b/;

function describeSpecifiers(list: string): { exported: string; local: string }[] {
  const out: { exported: string; local: string }[] = [];
  for (const part of list.split(",")) {
    const trimmed = part.trim();
    if (trimmed === "") continue;
    const asMatch = /^([A-Za-z_$][\w$]*)\s+as\s+([A-Za-z_$][\w$]*)$/.exec(trimmed);
    if (asMatch !== null) {
      out.push({ local: asMatch[1]!, exported: asMatch[2]! });
      continue;
    }
    if (/^[A-Za-z_$][\w$]*$/.test(trimmed)) out.push({ local: trimmed, exported: trimmed });
  }
  return out;
}

export function parseModuleExports(code: string, moduleId: string): ModuleExports {
  const scrubbed = code.replace(/export\s+type\s*\{[^}]*\}\s*;?/g, "").replace(/export\s+type\s+[A-Za-z_$][\w$]*[^;]*;/g, "");
  const explicit: Record<string, string> = {};
  for (const match of scrubbed.matchAll(ExportConst)) {
    explicit[match[1]!] = moduleId;
  }
  if (ExportDefault.test(scrubbed)) explicit.default = moduleId;
  for (const match of scrubbed.matchAll(ExportList)) {
    const from = match[3];
    for (const spec of describeSpecifiers(match[1] ?? "")) {
      explicit[spec.exported] = from ?? moduleId;
    }
  }
  const stars: string[] = [];
  for (const match of scrubbed.matchAll(ExportStar)) {
    stars.push(match[2]!);
  }
  return { explicit, stars };
}

export interface ExportGraphEnv {
  readFile(id: string): string | undefined;
  resolveSpec(spec: string, importer: string): string | undefined;
  virtualExports(id: string): readonly string[] | undefined;
}

const Interesting = ["routes", "default", "paths"] as const;

export function resolveAppMode(entryId: string, env: ExportGraphEnv): AppMode {
  const stack: string[] = [];
  const ambiguous: Record<string, string[]> = {};
  const resolved = (id: string): { names: Record<string, string>; starred: Record<string, true> } => {
    if (stack.includes(id)) return { names: {}, starred: {} };
    const virtual = env.virtualExports(id);
    if (virtual !== undefined) {
      const names: Record<string, string> = {};
      for (const name of virtual) names[name] = id;
      return { names, starred: {} };
    }
    const code = env.readFile(id);
    if (code === undefined) throw new Error(`[reze] cannot resolve the SSG entry graph at ${JSON.stringify(id)}`);
    stack.push(id);
    try {
      const parsed = parseModuleExports(code, id);
      const names: Record<string, string> = {};
      const starred: Record<string, true> = {};
      for (const [name, origin] of Object.entries(parsed.explicit)) {
        names[name] = origin === id ? id : (env.resolveSpec(origin, id) ?? missing(origin, id));
      }
      for (const star of parsed.stars) {
        const target = env.resolveSpec(star, id) ?? missing(star, id);
        const child = resolved(target);
        for (const [name, origin] of Object.entries(child.names)) {
          if (name === "default" || names[name] !== undefined) {
            if (name !== "default" && names[name] !== undefined && starred[name] === true && origin !== names[name]) {
              (ambiguous[name] ??= []).push(names[name]!, origin);
            }
            continue;
          }
          names[name] = origin;
          starred[name] = true;
        }
      }
      return { names, starred };
    } finally {
      stack.pop();
    }
  };
  const names = resolved(entryId).names;
  for (const name of Interesting) {
    const origins = [...new Set(ambiguous[name] ?? [])];
    if (origins.length > 1) {
      throw new Error(
        `[reze] SSG entry export ${JSON.stringify(name)} is ambiguous across origins: ${origins.map((origin) => JSON.stringify(origin)).join(", ")}`,
      );
    }
  }
  if (names.routes !== undefined) return { kind: "router", hasShell: names.default !== undefined };
  if (names.default !== undefined) return { kind: "standalone" };
  throw new Error("[reze] SSG entry must export a named `routes` table (router app) or a default component (standalone app)");
}

function missing(spec: string, importer: string): never {
  throw new Error(`[reze] cannot resolve ${JSON.stringify(spec)} from ${JSON.stringify(importer)} in the SSG entry graph`);
}
