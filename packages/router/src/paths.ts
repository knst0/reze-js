import type { PathsTree, RouteDefinition } from "./types";

type Kind = "static" | "param" | "optional" | "splat";

interface Spec {
  key: string;
  kind: Kind;
  name: string;
  text: string;
  fullPath: string;
  /** Node prefix without a trailing dynamic segment; the history base joins at the top. */
  base: string;
  /** The raw last pattern segment (`:id`, `*rest`, `""` for an index). */
  raw: string;
  /** Dynamic segments in the route's own path; builders bind one value, so more is a call-time error. */
  dynamics: number;
  matchable: boolean;
  children: Spec[];
}
function joinPaths(parent: string, child: string): string {
  if (child === "" || child === "/") return parent === "" ? "/" : parent;
  return parent.replace(/\/+$/, "") + "/" + child.replace(/^\/+/, "");
}

function childBase(base: string, parentFull: string, child: Spec): string {
  let rel = child.fullPath.slice(parentFull.length);
  if (child.kind !== "static") rel = rel.slice(0, rel.length - child.raw.length).replace(/\/+$/, "");
  return rel === "" || rel === "/" ? base : joinPaths(base, rel);
}

function capitalize(name: string): string {
  return name.slice(0, 1).toUpperCase() + name.slice(1);
}

function parseSegment(path: string): { kind: Kind; name: string; text: string; raw: string; dynamics: number } {
  const parts = path.split("/");
  let segment = "";
  for (let i = parts.length - 1; i >= 0; i--) {
    if (parts[i] !== "") {
      segment = parts[i]!;
      break;
    }
  }
  const empty = { kind: "static", name: "", text: "", raw: "" } as const;
  if (segment === "") return { ...empty, dynamics: 0 };
  const dynamics = parts.filter((part) => part !== "" && (part[0] === ":" || part[0] === "*")).length;
  const head = segment[0];
  if (head === ":") {
    const name = segment.endsWith("?") ? segment.slice(1, -1) : segment.slice(1);
    const kind: Kind = segment.endsWith("?") ? "optional" : "param";
    return { kind, name, text: "", raw: segment, dynamics };
  }
  if (head === "*") return { kind: "splat", name: segment.length === 1 ? "Rest" : segment.slice(1), text: "", raw: segment, dynamics };
  return { kind: "static", name: "", text: segment, raw: segment, dynamics };
}
function keyOf(segment: { kind: Kind; name: string; text: string }): string {
  if (segment.kind === "static") return segment.text === "" ? "index" : segment.text;
  return "by" + capitalize(segment.name);
}

/** Whether navigating to `fullPath` can match: a leaf, or an index child landing on the same path. */
function isMatchable(def: RouteDefinition, fullPath: string): boolean {
  if (def.children === undefined || def.children.length === 0) return true;
  return def.children.some((child) => {
    const childPath = joinPaths(fullPath, child.path);
    return childPath === fullPath && isMatchable(child, childPath);
  });
}

function specify(list: readonly RouteDefinition[], parentPath: string, trail: string): Spec[] {
  const specs: Spec[] = [];
  for (const def of list) {
    const segment = parseSegment(def.path);
    const fullPath = joinPaths(parentPath, def.path);
    const key = def.name ?? keyOf(segment);
    if (specs.some((spec) => spec.key === key)) {
      throw new Error(`[reze-router] duplicate path name "${key}" under "${trail || "/"}"; rename a segment`);
    }
    specs.push({
      key,
      kind: segment.kind,
      name: segment.name,
      text: segment.text,
      fullPath,
      raw: segment.raw,
      dynamics: segment.dynamics,
      base: segment.kind === "static" ? fullPath : fullPath.slice(0, fullPath.length - segment.raw.length).replace(/\/+$/, ""),
      matchable: isMatchable(def, fullPath),
      children: specify(def.children ?? [], fullPath, `${trail}/${key}`),
    });
  }
  return specs;
}

function isSearch(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function printable(value: unknown, what: string): string {
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") return String(value);
  throw new Error(`[reze-router] ${what} must be a string, number or boolean, got ${Array.isArray(value) ? "array" : typeof value}`);
}
function renderHref(path: string, search: unknown, hash: unknown): string {
  let out = path;
  if (search !== undefined) {
    if (!isSearch(search)) throw new Error(`[reze-router] paths search must be an object, got ${typeof search}`);
    const qs = new URLSearchParams();
    for (const key in search) {
      const value = search[key];
      if (value === null || value === undefined) continue;
      if (Array.isArray(value)) {
        for (const item of value) {
          if (item !== null && item !== undefined) qs.append(key, printable(item, `search value for "${key}"`));
        }
      } else {
        qs.set(key, printable(value, `search value for "${key}"`));
      }
    }
    const text = qs.toString();
    if (text !== "") out += "?" + text;
  }
  if (hash !== undefined) {
    if (typeof hash !== "string") throw new Error(`[reze-router] paths hash must be a string, got ${typeof hash}`);
    if (hash !== "") out += "#" + (hash.startsWith("#") ? hash.slice(1) : hash);
  }
  return out;
}

function appendValue(base: string, spec: Spec, trail: string, value: unknown): string {
  if (value === undefined || value === null) {
    if (spec.kind === "param") throw new Error(`[reze-router] paths.${trail} requires a value for ":${spec.name}"`);
    return base;
  }
  if (spec.kind === "splat") {
    const parts = typeof value === "string" ? value.split("/") : Array.isArray(value) ? value : undefined;
    if (parts === undefined) throw new Error(`[reze-router] paths.${trail} takes a string or an array, got ${typeof value}`);
    const encoded = parts.map((part) => encodeURIComponent(printable(part, `paths.${trail} part`))).join("/");
    return encoded === "" ? base : base + "/" + encoded;
  }
  return base + "/" + encodeURIComponent(printable(value, `paths.${trail} value`));
}

function attach(target: object, key: string, child: unknown): void {
  Object.defineProperty(target, key, { value: child, enumerable: true });
}

function buildNode(base: string, spec: Spec, trail: string): unknown {
  if (spec.kind === "static" && !spec.matchable) {
    const branch: Record<string, unknown> = {};
    Object.defineProperty(branch, "toString", { value: () => base });
    for (const child of spec.children) {
      attach(branch, child.key, buildNode(childBase(base, spec.fullPath, child), child, `${trail}.${child.key}`));
    }
    return branch;
  }
  const node = (...args: readonly unknown[]): unknown => {
    if (spec.kind === "static") {
      const [search, hash] = args;
      return renderHref(base, search, hash);
    }
    if (spec.dynamics > 1)
      throw new Error(`[reze-router] paths.${trail} has ${spec.dynamics} dynamic segments; split "${spec.fullPath}" into nested routes`);
    const [value, ...tail] = args;
    const path = appendValue(base, spec, trail, value);
    if (spec.children.length > 0) {
      if (tail.length > 0) throw new Error(`[reze-router] paths.${trail} takes only a value; call the returned node`);
      return buildNode(path, { ...spec, kind: "static" as const }, trail);
    }
    const [search, hash] = tail;
    return renderHref(path, search, hash);
  };
  Object.defineProperty(node, "toString", { value: () => base });
  for (const child of spec.children) {
    attach(node, child.key, buildNode(childBase(base, spec.fullPath, child), child, `${trail}.${child.key}`));
  }
  return node;
}

/** Builds the `paths` builders from a route table: static segments are props, params are `byName` calls. `base` prefixes every emitted href, so builders return `<a href>`-ready strings. Shared by every router of one factory. */
export function buildPaths(defs: readonly RouteDefinition[], base = ""): PathsTree {
  const tree: Record<string, unknown> = {};
  for (const spec of specify(defs, "", "")) {
    attach(tree, spec.key, buildNode(base + spec.base, spec, spec.key));
  }
  // Runtime nodes satisfy the loose contract positionally (calls return strings or nodes).
  return tree as unknown as PathsTree;
}
