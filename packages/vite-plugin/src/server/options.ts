import { isAbsolute, relative, sep } from "node:path";

export type StaticParams = Readonly<Record<string, string | readonly string[] | undefined>>;

export type StaticPathsValue = readonly StaticParams[] | (() => readonly StaticParams[] | PromiseLike<readonly StaticParams[]>);

export interface SsgOptions {
  document?: string;
  trailingSlash?: "always" | "never";
  timeoutMs?: number;
  paths?: Readonly<Record<string, StaticPathsValue>>;
  /** Prerender up to the first async boundary and stream the rest from `ssr` at request time. */
  ppr?: boolean;
}

export interface ResolvedSsgOptions {
  document: string;
  trailingSlash: "always" | "never";
  timeoutMs: number;
  paths: Readonly<Record<string, StaticPathsValue>>;
  ppr: boolean;
}

export type DevContext = (request: Request) => unknown;

export interface SsrOptions {
  document?: string;
  timeoutMs?: number;
  outDir?: string;
  devContext?: DevContext;
}

export interface ResolvedSsrOptions {
  document: string;
  timeoutMs: number;
  outDir: string;
  devContext: DevContext | undefined;
}

const TimeoutDefault = 30_000;
const OutDirDefault = "dist/server";
const DocumentDefault = "src/document.tsx";
export const ClientOutDirDefault = "dist/client";

function checkObject(prefix: string, input: unknown): void {
  if (typeof input !== "object" || input === null || Array.isArray(input)) {
    throw new Error(`[reze] ${prefix} must be true or an options object, got ${JSON.stringify(input)}`);
  }
  if ("entry" in input) {
    throw new Error(
      `[reze] ${prefix}.entry is gone: the app is the file routes in src/routes and shared markup is in the document; remove ${prefix}.entry and the entry module`,
    );
  }
}

function checkTimeout(prefix: string, timeoutMs: unknown): void {
  if (typeof timeoutMs !== "number" || !Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error(`[reze] ${prefix}.timeoutMs must be a finite number above zero, got ${JSON.stringify(timeoutMs)}`);
  }
}

export function resolveSsgOptions(option: true | SsgOptions): ResolvedSsgOptions {
  const input = option === true ? {} : option;
  checkObject("ssg", input);
  const trailingSlash = input.trailingSlash ?? "always";
  if (trailingSlash !== "always" && trailingSlash !== "never") {
    throw new Error(`[reze] ssg.trailingSlash must be "always" or "never", got ${JSON.stringify(trailingSlash)}`);
  }
  const timeoutMs = input.timeoutMs ?? TimeoutDefault;
  checkTimeout("ssg", timeoutMs);
  if (input.ppr !== undefined && typeof input.ppr !== "boolean") {
    throw new Error(`[reze] ssg.ppr must be a boolean, got ${JSON.stringify(input.ppr)}`);
  }
  if (input.paths !== undefined && (typeof input.paths !== "object" || input.paths === null || Array.isArray(input.paths))) {
    throw new Error("[reze] ssg.paths must map route patterns to param lists or listing callbacks");
  }
  return {
    document: input.document ?? DocumentDefault,
    trailingSlash,
    timeoutMs,
    paths: input.paths ?? {},
    ppr: input.ppr ?? false,
  };
}

export function resolveSsrOptions(option: true | SsrOptions): ResolvedSsrOptions {
  const input = option === true ? {} : option;
  checkObject("ssr", input);
  if (input.devContext !== undefined && typeof input.devContext !== "function") {
    throw new Error("[reze] ssr.devContext must be a function of the request that returns the base router context");
  }
  const timeoutMs = input.timeoutMs ?? TimeoutDefault;
  checkTimeout("ssr", timeoutMs);
  return {
    document: input.document ?? DocumentDefault,
    timeoutMs,
    outDir: input.outDir ?? OutDirDefault,
    devContext: input.devContext,
  };
}

export function assertSharedServerOptions(ssg: ResolvedSsgOptions, ssr: ResolvedSsrOptions): void {
  if (ssg.document !== ssr.document) {
    throw new Error("[reze] ssg and ssr must use the same document");
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

export function assertDisjointOutDirs(clientOutDir: string, ssrOutDir: string): void {
  if (isInside(clientOutDir, ssrOutDir) || isInside(ssrOutDir, clientOutDir)) {
    throw new Error(
      `[reze] ssr.outDir ${JSON.stringify(ssrOutDir)} overlaps build.outDir ${JSON.stringify(clientOutDir)}: the server bundle replaces its directory, and build.outDir is served to browsers. Set ssr.outDir to a separate directory, such as "dist/server"`,
    );
  }
}

function isInside(parent: string, child: string): boolean {
  const rel = relative(parent, child);
  return rel === "" || (rel !== ".." && !rel.startsWith(`..${sep}`) && !isAbsolute(rel));
}
