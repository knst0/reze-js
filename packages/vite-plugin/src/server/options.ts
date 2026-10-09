export type StaticParams = Readonly<Record<string, string | readonly string[] | undefined>>;

export type StaticPathsValue = readonly StaticParams[] | (() => readonly StaticParams[] | PromiseLike<readonly StaticParams[]>);

export interface SsgOptions {
  entry: string;
  template?: string;
  selector?: `#${string}`;
  trailingSlash?: "always" | "never";
  timeoutMs?: number;
  paths?: Readonly<Record<string, StaticPathsValue>>;
}

export interface ResolvedSsgOptions {
  entry: string;
  template: string;
  selector: `#${string}`;
  rootId: string;
  trailingSlash: "always" | "never";
  timeoutMs: number;
  paths: Readonly<Record<string, StaticPathsValue>>;
}

export interface SsrOptions {
  entry: string;
  template?: string;
  selector?: `#${string}`;
  timeoutMs?: number;
  outDir?: string;
}

export interface ResolvedSsrOptions {
  entry: string;
  template: string;
  selector: `#${string}`;
  rootId: string;
  timeoutMs: number;
  outDir: string;
}

export const SelectorPattern = /^#[A-Za-z_][A-Za-z0-9_-]*$/;

const TimeoutDefault = 30_000;
const OutDirDefault = "dist-server";

function checkEntry(prefix: string, entry: unknown): void {
  if (typeof entry !== "string" || entry.length === 0) {
    throw new Error(`[reze] ${prefix}.entry is required: a project-relative module exporting the application`);
  }
}

function checkSelector(prefix: string, selector: string): void {
  if (!SelectorPattern.test(selector)) {
    throw new Error(`[reze] ${prefix}.selector must be "#" plus an ASCII identifier, got ${JSON.stringify(selector)}`);
  }
}

function checkTimeout(prefix: string, timeoutMs: unknown): void {
  if (typeof timeoutMs !== "number" || !Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error(`[reze] ${prefix}.timeoutMs must be a finite number above zero, got ${JSON.stringify(timeoutMs)}`);
  }
}

export function resolveSsgOptions(input: SsgOptions): ResolvedSsgOptions {
  checkEntry("ssg", input.entry);
  const selector = input.selector ?? "#app";
  checkSelector("ssg", selector);
  const trailingSlash = input.trailingSlash ?? "always";
  if (trailingSlash !== "always" && trailingSlash !== "never") {
    throw new Error(`[reze] ssg.trailingSlash must be "always" or "never", got ${JSON.stringify(trailingSlash)}`);
  }
  const timeoutMs = input.timeoutMs ?? TimeoutDefault;
  checkTimeout("ssg", timeoutMs);
  if (input.paths !== undefined && (typeof input.paths !== "object" || input.paths === null || Array.isArray(input.paths))) {
    throw new Error("[reze] ssg.paths must map route patterns to param lists or listing callbacks");
  }
  return {
    entry: input.entry,
    template: input.template ?? "index.html",
    selector,
    rootId: selector.slice(1),
    trailingSlash,
    timeoutMs,
    paths: input.paths ?? {},
  };
}

export function resolveSsrOptions(input: SsrOptions): ResolvedSsrOptions {
  checkEntry("ssr", input.entry);
  const selector = input.selector ?? "#app";
  checkSelector("ssr", selector);
  const timeoutMs = input.timeoutMs ?? TimeoutDefault;
  checkTimeout("ssr", timeoutMs);
  return {
    entry: input.entry,
    template: input.template ?? "index.html",
    selector,
    rootId: selector.slice(1),
    timeoutMs,
    outDir: input.outDir ?? OutDirDefault,
  };
}

export function assertSharedServerOptions(ssg: ResolvedSsgOptions, ssr: ResolvedSsrOptions): void {
  if (ssg.entry !== ssr.entry || ssg.template !== ssr.template || ssg.selector !== ssr.selector) {
    throw new Error("[reze] ssg and ssr must use the same entry, template and selector");
  }
}

export async function resolvePathsCallbacks(
  paths: Readonly<Record<string, StaticPathsValue>>,
): Promise<Readonly<Record<string, readonly StaticParams[]>>> {
  const out: Record<string, readonly StaticParams[]> = {};
  for (const [pattern, value] of Object.entries(paths)) {
    const list = typeof value === "function" ? await value() : value;
    if (!Array.isArray(list)) {
      throw new Error(`[reze] ssg.paths[${JSON.stringify(pattern)}] must resolve to an array of param records`);
    }
    for (const [index, entry] of list.entries()) {
      if (typeof entry !== "object" || entry === null || Array.isArray(entry)) {
        throw new Error(`[reze] ssg.paths[${JSON.stringify(pattern)}][${index}] must be a record of params`);
      }
      for (const [name, param] of Object.entries(entry)) {
        if (param !== undefined && typeof param !== "string" && !Array.isArray(param)) {
          throw new Error(
            `[reze] ssg.paths[${JSON.stringify(pattern)}][${index}][${JSON.stringify(name)}] must be a string, an array of strings, or undefined`,
          );
        }
        if (Array.isArray(param) && param.some((part) => typeof part !== "string")) {
          throw new Error(`[reze] ssg.paths[${JSON.stringify(pattern)}][${index}][${JSON.stringify(name)}] must hold strings only`);
        }
      }
    }
    out[pattern] = list;
  }
  return out;
}
