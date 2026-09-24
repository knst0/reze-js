/** One segment of a route pattern. */
export type Segment =
  | { kind: "static"; text: string }
  | { kind: "param"; name: string; isOptional: boolean }
  | { kind: "rest"; name: string };

/**
 * A route pattern split once into segments, with its rank: static segments outrank params,
 * params outrank optional ones, and at equal specificity a pattern without `*rest` wins.
 */
export interface Pattern {
  segments: Segment[];
  score: number;
}

const StaticScore = 3;
const ParamScore = 2;
const OptionalScore = 1;

/** Path segments without empty ones: `/a//b/` is `["a", "b"]`. */
export function pathSegments(path: string): string[] {
  return path.split("/").filter((segment) => segment !== "");
}

/** `/users/:id`, `:id?` optional, `*rest` for the remaining segments (last only). */
export function compilePattern(path: string): Pattern {
  const segments: Segment[] = pathSegments(path).map((part) => {
    if (part.startsWith("*")) return { kind: "rest", name: part.slice(1) || "*" };
    if (part.startsWith(":")) {
      const isOptional = part.endsWith("?");
      return { kind: "param", name: part.slice(1, isOptional ? -1 : undefined), isOptional };
    }
    return { kind: "static", text: decodeURIComponent(part) };
  });
  const specificity = segments.reduce((total, segment) => {
    if (segment.kind === "static") return total + StaticScore;
    if (segment.kind === "rest") return total;
    return total + (segment.isOptional ? OptionalScore : ParamScore);
  }, 0);
  const isExact = segments.every((segment) => segment.kind !== "rest");
  return { segments, score: specificity * 2 + (isExact ? 1 : 0) };
}

/** The params `pattern` binds for `path`, or `undefined` when it does not match all of it. */
export function matchPattern(pattern: Pattern, path: string): Record<string, string> | undefined {
  const parts = pathSegments(path);
  const params: Record<string, string> = {};
  let index = 0;
  for (const segment of pattern.segments) {
    if (segment.kind === "rest") {
      params[segment.name] = parts.slice(index).map(decodeURIComponent).join("/");
      return params;
    }
    const part = parts[index];
    if (segment.kind === "static") {
      if (part === undefined || decodeURIComponent(part) !== segment.text) return undefined;
      index++;
      continue;
    }
    if (part === undefined) {
      if (!segment.isOptional) return undefined;
      continue;
    }
    params[segment.name] = decodeURIComponent(part);
    index++;
  }
  return index === parts.length ? params : undefined;
}
