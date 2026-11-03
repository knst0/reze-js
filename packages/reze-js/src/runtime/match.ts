export type Segment = { kind: "static"; name: string } | { kind: "param" | "optional" | "splat"; name: string };

interface Matchable {
  segments: Segment[];
}

const rank = { static: 0, param: 1, optional: 2, splat: 3 };

export function compare(a: Matchable, b: Matchable): number {
  for (let index = 0; index < Math.max(a.segments.length, b.segments.length); index++) {
    const left = a.segments[index];
    const right = b.segments[index];
    if (!left || !right) return left ? 1 : -1;
    const order = rank[left.kind] - rank[right.kind];
    if (order !== 0) return order;
  }
  return 0;
}

/** Matches `parts` against `route`, and returns the params and the canonical path. */
export function match(route: Matchable, parts: string[]): [Record<string, string | undefined>, string] | null {
  const params: Record<string, string | undefined> = {};
  const canonical: string[] = [];
  let index = 0;
  for (const segment of route.segments) {
    const part = parts[index];
    switch (segment.kind) {
      case "static":
        if (part?.toLowerCase() !== segment.name) return null;
        canonical.push(segment.name);
        index++;
        break;
      case "param":
        if (part === undefined) return null;
        params[segment.name] = decodeURIComponent(part);
        canonical.push(part);
        index++;
        break;
      case "optional":
        if (part !== undefined) {
          params[segment.name] = decodeURIComponent(part);
          canonical.push(part);
          index++;
        }
        break;
      case "splat":
        if (index >= parts.length) return null;
        params[segment.name] = parts.slice(index).map(decodeURIComponent).join("/");
        canonical.push(...parts.slice(index));
        index = parts.length;
        break;
    }
  }
  return index === parts.length ? [params, `/${canonical.join("/")}`] : null;
}
