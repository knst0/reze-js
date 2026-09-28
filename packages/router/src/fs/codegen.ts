import { dirname, posix, relative } from "node:path";

import type { FileRoute } from "./scan";

const StringType = "${string}";

/** The `virtual:reze-routes` module: a nested route table importing each file under `dir` (absolute) lazily. */
/** The `paths` builders in `virtual:reze-routes`: prebuilt string closures the bundler drops when unimported. */
export function pathsModule(routes: readonly FileRoute[], base: string): string {
  if (routes.length === 0) return "export const paths = {};\n";
  const helpers = `const enc = encodeURIComponent;
const href = (path, search, hash) => {
  let out = path;
  if (search !== undefined) {
    const qs = new URLSearchParams();
    for (const key in search) {
      const value = search[key];
      if (value === null || value === undefined) continue;
      if (Array.isArray(value)) {
        for (const item of value) {
          if (item !== null && item !== undefined) qs.append(key, String(item));
        }
      } else qs.set(key, String(value));
    }
    const query = qs.toString();
    if (query !== "") out += "?" + query;
  }
  if (typeof hash === "string" && hash !== "") out += "#" + (hash.startsWith("#") ? hash.slice(1) : hash);
  return out;
};
const splat = (base, parts) => {
  const list = typeof parts === "string" ? parts.split("/") : (parts ?? []);
  const rest = list.map((part) => enc(String(part))).join("/");
  return rest === "" ? base : base + "/" + rest;
};
const node = (fn, children) => Object.assign(fn, children);
`;
  const emit = (list: readonly FileRoute[], depth: number): string =>
    `{ ${list.map((route) => `${JSON.stringify(route.name)}: ${emitNode(route, base, depth, route.name)}`).join(", ")} }`;
  return `${helpers}export const paths = ${emit(routes, 0)};\n`;
}

/** Href prefix of `fullPath` as a template literal: dynamics encode bound vars, a mid-path optional renders conditionally with its slash. */
function prefixTemplate(fullPath: string): string {
  let counter = 0;
  const vars = new Map<string, string>();
  const nameOf = (segment: string): string => {
    let name = vars.get(segment);
    if (name === undefined) {
      name = counter === 0 ? "value" : "value" + (counter + 1);
      counter++;
      vars.set(segment, name);
    }
    return name;
  };
  let out = "";
  for (const segment of fullPath.split("/")) {
    if (segment === "") continue;
    const head = segment[0];
    if (head !== ":" && head !== "*") {
      out += "/" + escapeTemplate(segment);
    } else if (head === ":" && segment.endsWith("?")) {
      const name = nameOf(segment);
      out += "${(" + name + " === undefined || " + name + ' === null ? "" : "/" + enc(' + name + "))}";
    } else {
      out += "/${enc(" + nameOf(segment) + ")}";
    }
  }
  return "`" + out + "`";
}

function splitBase(fullPath: string): { base: string; segment: string } {
  const index = fullPath.lastIndexOf("/");
  return { base: fullPath.slice(0, index), segment: fullPath.slice(index + 1) };
}

function countDynamics(fullPath: string): number {
  let count = 0;
  for (const segment of fullPath.split("/")) {
    if (segment !== "" && (segment[0] === ":" || segment[0] === "*")) count++;
  }
  return count;
}

function emitNode(route: FileRoute, base: string, depth: number, trail: string): string {
  const { segment } = splitBase(route.fullPath);
  const head = segment[0];
  const kind: SegmentKind = head === "*" ? "splat" : head === ":" ? (segment.endsWith("?") ? "optional" : "param") : "static";
  const arg = depth === 0 ? "value" : "value" + (depth + 1);
  const kids = route.children
    .map(
      (child) => JSON.stringify(child.name) + ": " + emitNode(child, base, depth + (kind === "static" ? 0 : 1), trail + "." + child.name),
    )
    .join(", ");
  const wrap = (fn: string): string => (kids === "" ? fn : "/*@__PURE__*/ node(" + fn + ", { " + kids + " })");
  const literal = JSON.stringify(base + route.fullPath);
  if (kind === "static") {
    if (!isCallable(route)) return "{ toString: () => " + literal + ", " + kids + " }";
    return wrap("(search, hash) => href(" + literal + ", search, hash)");
  }
  if (countDynamics(route.fullPath) - depth > 1) {
    const message = JSON.stringify("[reze-router] paths." + trail + " has multiple dynamic segments; split it into nested routes");
    return "() => { throw new Error(" + message + "); }";
  }
  const template = prefixTemplate(base + route.fullPath);
  if (route.children.length === 0) {
    if (kind === "splat") {
      const at = JSON.stringify(base + splitBase(route.fullPath).base);
      return "(parts, search, hash) => href(splat(" + at + ", parts), search, hash)";
    }
    return "(" + arg + ", search, hash) => href(" + template + ", search, hash)";
  }
  if (kind === "splat") throw new Error("[reze-router] unreachable splat with children in " + route.file);
  if (!isCallable(route)) return "(" + arg + ") => ({ toString: () => " + template + ", " + kids + " })";
  return "(" + arg + ") => " + wrap("(search, hash) => href(" + template + ", search, hash)");
}
/** The `virtual:reze-routes` module: a nested lazy route table plus prebuilt `paths` builders, both dropping from the bundle when unimported. */
export function routesModule(routes: readonly FileRoute[], dir: string, base: string): string {
  const root = dir.replace(/\\/g, "/");
  const emit = (list: readonly FileRoute[]): string =>
    "[" +
    list
      .map(
        (route) =>
          `{ path: ${JSON.stringify(route.path)}, load: () => import(${JSON.stringify(posix.join(root, route.file))})` +
          (route.children.length > 0 ? `, children: ${emit(route.children)}` : "") +
          " }",
      )
      .join(", ") +
    "]";
  return `export const routes = ${emit(routes)};\n${pathsModule(routes, base)}`;
}

function escapeTemplate(text: string): string {
  return text.replace(/[`\\]|\$\{/g, (match) => "\\" + match);
}

/** Href shapes of `fullPath`; a splat without a static segment before it (a catch-all) adds no `${string}` shape. */
function hrefVariants(fullPath: string): string[][] {
  let variants: string[][] = [[]];
  let hasStatic = false;
  for (const segment of fullPath.split("/")) {
    if (segment === "") continue;
    const head = segment[0];
    if (head === "*" && !hasStatic) continue;
    if (head === "*" || (head === ":" && segment.endsWith("?"))) {
      variants = [...variants, ...variants.map((parts) => [...parts, StringType])];
    } else {
      hasStatic ||= head !== ":";
      const part = head === ":" ? StringType : segment;
      variants = variants.map((parts) => [...parts, part]);
    }
  }
  return variants;
}

function renderPath(parts: readonly string[], base = ""): string {
  if (!parts.includes(StringType)) return JSON.stringify(base + "/" + parts.join("/"));
  return "`" + base + "/" + parts.map((part) => (part === StringType ? part : escapeTemplate(part))).join("/") + "`";
}

function collectPaths(routes: readonly FileRoute[], paths: Set<string>): void {
  for (const route of routes) {
    if (route.children.length > 0) collectPaths(route.children, paths);
    else for (const parts of hrefVariants(route.fullPath)) paths.add(renderPath(parts));
  }
}

interface Leaf {
  fullPath: string;
  file: string;
}

function collectLeaves(routes: readonly FileRoute[], leaves: Leaf[]): void {
  for (const route of routes) {
    if (route.children.length > 0) collectLeaves(route.children, leaves);
    else leaves.push({ fullPath: route.fullPath, file: route.file });
  }
}

function paramsType(fullPath: string): string {
  const props: string[] = [];
  for (const segment of fullPath.split("/")) {
    if (segment === "") continue;
    const head = segment[0];
    if (head !== ":" && head !== "*") continue;
    const isOptional = head === ":" && segment.endsWith("?");
    const name = segment.slice(1, isOptional ? -1 : undefined);
    props.push(`readonly ${JSON.stringify(name)}${isOptional ? "?" : ""}: string`);
  }
  return props.length === 0 ? "{}" : `{ ${props.join("; ")} }`;
}

/** Extensionless specifier of `file` (routes-relative) from the generated `.d.ts`. */
function specifier(dtsFile: string, routesDir: string, file: string): string {
  const from = dirname(dtsFile).replace(/\\/g, "/");
  const to = routesDir.replace(/\\/g, "/");
  const rel = relative(from, to).replace(/\\/g, "/");
  const path = (rel === "" ? "." : rel) + "/" + file.replace(/\.[jt]sx?$/, "");
  return path.startsWith(".") ? path : "./" + path;
}

type SegmentKind = "static" | "param" | "optional" | "splat";

function lastKind(path: string): SegmentKind {
  const parts = path.split("/");
  let segment = "";
  for (let i = parts.length - 1; i >= 0; i--) {
    if (parts[i] !== "") {
      segment = parts[i]!;
      break;
    }
  }
  if (segment.startsWith("*")) return "splat";
  if (segment.startsWith(":")) return segment.endsWith("?") ? "optional" : "param";
  return "static";
}

/** Whether navigating to the route's own `fullPath` can match: a leaf, or an index child landing on it. */
function isCallable(route: FileRoute): boolean {
  return route.children.length === 0 || route.children.some((child) => child.path === "/");
}

interface EmitState {
  search: boolean;
  href: boolean;
}

/** Every href shape the route's builders return, as `HrefFor` members: one per optional/splat expansion. */
function hrefFor(fullPath: string, base: string, state: EmitState): string {
  state.href = true;
  const shapes = hrefVariants(fullPath).map((parts) => renderPath(parts, base));
  return shapes.map((shape) => `HrefFor<${shape}>`).join(" | ");
}

function staticNode(route: FileRoute, base: string, state: EmitState): string {
  const parts: string[] = [];
  if (isCallable(route)) {
    state.search = true;
    parts.push(`(search?: SearchInit, hash?: string): ${hrefFor(route.fullPath, base, state)}`);
  } else {
    parts.push("toString(): string");
  }
  for (const child of route.children) {
    parts.push(`readonly ${JSON.stringify(child.name)}: ${nodeType(child, base, state)}`);
  }
  return `{ ${parts.join("; ")} }`;
}
function nodeType(route: FileRoute, base: string, state: EmitState): string {
  const kind = lastKind(route.path);
  if (kind === "static") return staticNode(route, base, state);
  if (route.children.length === 0) {
    state.search = true;
    const returns = hrefFor(route.fullPath, base, state);
    if (kind === "splat") return `{ (parts?: string | readonly (string | number)[], search?: SearchInit, hash?: string): ${returns} }`;
    const value = kind === "optional" ? "value?: string | number" : "value: string | number";
    return `{ (${value}, search?: SearchInit, hash?: string): ${returns} }`;
  }
  const value = kind === "optional" ? "value?: string | number" : "value: string | number";
  return `{ (${value}): ${staticNode(route, base, state)} }`;
}

/** The generated `.d.ts`: types `virtual:reze-routes` and registers leaf patterns with params and data, the `paths` builders, every leaf href shape, and `base`. */
export function routesDts(routes: readonly FileRoute[], base: string, dtsFile: string, routesDir: string): string {
  const set = new Set<string>();
  collectPaths(routes, set);
  const paths = set.size === 0 ? "never" : [...set].sort().join(" | ");
  const leaves: Leaf[] = [];
  collectLeaves(routes, leaves);
  const state: EmitState = { search: false, href: false };
  const entries = leaves.map(
    (leaf) =>
      `        ${JSON.stringify(leaf.fullPath)}: { params: ${paramsType(leaf.fullPath)}; data: DataOf<import(${JSON.stringify(specifier(dtsFile, routesDir, leaf.file))})> };`,
  );
  const tree =
    routes.length === 0
      ? "{}"
      : `{ ${routes.map((route) => `readonly ${JSON.stringify(route.name)}: ${nodeType(route, base, state)}`).join("; ")} }`;
  const header = `${leaves.length === 0 ? "" : '  import type { DataOf } from "@rezejs/router";\n'}${state.search ? '  import type { SearchInit } from "@rezejs/router";\n' : ""}${state.href ? '  import type { HrefFor } from "@rezejs/router";\n' : ""}`;
  return `// Generated by @rezejs/vite-plugin. Do not edit.
declare module "virtual:reze-routes" {
  import type { PathsTree, RouteDefinition } from "@rezejs/router";
  export const routes: readonly RouteDefinition[];
  export const paths: PathsTree;
}
declare module "virtual:reze-routes/register" {
${header}  module "@rezejs/router" {
    interface Register {
      paths: ${paths};
      base: ${JSON.stringify(base)};
      routes: ${entries.length === 0 ? "{}" : `{\n${entries.join("\n")}\n      }`};
      pathsTree: ${tree};
    }
  }
}
`;
}
