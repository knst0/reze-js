import { ParamName, SplatName } from "../match";

export interface FileRoute {
  /** File path relative to the routes directory, without extension. */
  id: string;
  /** File path relative to the routes directory. */
  file: string;
  /** Pattern relative to the parent route. */
  path: string;
  fullPath: string;
  children: FileRoute[];
}

const RouteExtension = /\.[jt]sx?$/;
const Ignored = /\.d\.ts$|\.(?:test|spec)\.[^/]*$|(?:^|\/)\./;

function mapSegment(segment: string, isLast: boolean): string {
  if (isLast && segment === "index") return "";
  if (segment.startsWith("(") && segment.endsWith(")")) return "";
  if (segment.startsWith("[[") && segment.endsWith("]]")) return ":" + segment.slice(2, -2) + "?";
  if (segment.startsWith("[...") && segment.endsWith("]")) return "*" + segment.slice(4, -1);
  if (segment.startsWith("[") && segment.endsWith("]")) return ":" + segment.slice(1, -1);
  return segment;
}

function joinPaths(parent: string, child: string): string {
  if (child === "/") return parent === "" ? "/" : parent;
  return parent === "/" ? child : parent + child;
}

function validate(route: FileRoute): void {
  const fail = (reason: string): Error => new Error(`[reze-router] ${route.file}: ${reason} in "${route.fullPath}"`);
  const segments = route.fullPath.split("/").filter((segment) => segment !== "");
  const names = new Set<string>();
  let isOptionalSeen = false;
  for (let i = 0; i < segments.length; i++) {
    const segment = segments[i]!;
    const head = segment[0];
    if (head !== ":" && head !== "*") {
      if (isOptionalSeen) throw fail("an optional param must not precede a required segment");
      continue;
    }
    const isOptional = head === ":" && segment.endsWith("?");
    const name = segment.slice(1, isOptional ? -1 : undefined);
    if (head === "*" && i !== segments.length - 1) throw fail(`splat "${name}" must be the last segment`);
    if (!(head === "*" ? SplatName : ParamName).test(name)) throw fail(`invalid param name "${name}"`);
    if (names.has(name)) throw fail(`duplicate param "${name}"`);
    names.add(name);
    if (isOptional) isOptionalSeen = true;
    else if (head === ":" && isOptionalSeen) throw fail("an optional param must not precede a required segment");
  }
}

/** The pathname shapes `fullPath` matches, as the matcher compares them: case-insensitive, param and splat names erased, optionals expanded. */
function shapesOf(fullPath: string): string[] {
  let shapes = [""];
  for (const segment of fullPath.split("/")) {
    if (segment === "") continue;
    const head = segment[0];
    if (head === ":" && segment.endsWith("?")) shapes = [...shapes, ...shapes.map((shape) => shape + "/:")];
    else shapes = shapes.map((shape) => shape + "/" + (head === ":" ? ":" : head === "*" ? "*" : segment.toLowerCase()));
  }
  return shapes;
}

function checkConflicts(routes: readonly FileRoute[], leaves: Map<string, FileRoute>): void {
  for (const route of routes) {
    if (route.children.length > 0) {
      checkConflicts(route.children, leaves);
      continue;
    }
    for (const shape of shapesOf(route.fullPath)) {
      const other = leaves.get(shape);
      if (other !== undefined) throw new Error(`[reze-router] routes "${other.file}" and "${route.file}" both match "${route.fullPath}"`);
      leaves.set(shape, route);
    }
  }
}

/**
 * Builds the route tree from `files`, posix paths relative to the routes directory, using the nested convention:
 * `index`, `(group)`, `[param]`, `[[optional]]`, `[...splat]`; a file named like a directory is that directory's layout.
 * Children are sorted by `id`. Throws on duplicate, conflicting or invalid routes.
 */
export function scanRoutes(files: readonly string[]): FileRoute[] {
  const byId = new Map<string, FileRoute>();
  for (const file of files) {
    if (!RouteExtension.test(file) || Ignored.test(file)) continue;
    const id = file.replace(RouteExtension, "");
    const existing = byId.get(id);
    if (existing !== undefined) {
      const [a, b] = [existing.file, file].sort();
      throw new Error(`[reze-router] duplicate route files for "${id}": ${a}, ${b}`);
    }
    byId.set(id, { id, file, path: "/", fullPath: "/", children: [] });
  }
  const roots: FileRoute[] = [];
  for (const id of [...byId.keys()].sort()) {
    const route = byId.get(id)!;
    const segments = id.split("/");
    let parentDepth = segments.length - 1;
    while (parentDepth > 0 && !byId.has(segments.slice(0, parentDepth).join("/"))) parentDepth--;
    const parent = parentDepth > 0 ? byId.get(segments.slice(0, parentDepth).join("/")) : undefined;
    const own = segments.slice(parentDepth);
    const mapped = own.map((segment, i) => mapSegment(segment, i === own.length - 1)).filter((segment) => segment !== "");
    route.path = "/" + mapped.join("/");
    route.fullPath = joinPaths(parent?.fullPath ?? "", route.path);
    validate(route);
    (parent?.children ?? roots).push(route);
  }
  checkConflicts(roots, new Map());
  return roots;
}
