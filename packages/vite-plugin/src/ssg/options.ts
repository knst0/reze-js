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

export const SelectorPattern = /^#[A-Za-z_][A-Za-z0-9_-]*$/;

const TimeoutDefault = 30_000;

export function resolveSsgOptions(input: SsgOptions, root: string): ResolvedSsgOptions {
  if (typeof input.entry !== "string" || input.entry.length === 0) {
    throw new Error("[reze] ssg.entry is required: a project-relative module exporting the application");
  }
  const selector = input.selector ?? "#app";
  if (!SelectorPattern.test(selector)) {
    throw new Error(`[reze] ssg.selector must be "#" plus an ASCII identifier, got ${JSON.stringify(selector)}`);
  }
  const trailingSlash = input.trailingSlash ?? "always";
  if (trailingSlash !== "always" && trailingSlash !== "never") {
    throw new Error(`[reze] ssg.trailingSlash must be "always" or "never", got ${JSON.stringify(trailingSlash)}`);
  }
  const timeoutMs = input.timeoutMs ?? TimeoutDefault;
  if (typeof timeoutMs !== "number" || !Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new Error(`[reze] ssg.timeoutMs must be a finite number above zero, got ${JSON.stringify(timeoutMs)}`);
  }
  if (input.paths !== undefined && (typeof input.paths !== "object" || input.paths === null || Array.isArray(input.paths))) {
    throw new Error("[reze] ssg.paths must map route patterns to param lists or listing callbacks");
  }
  void root;
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
