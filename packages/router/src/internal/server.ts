import type { JSX } from "@rezejs/dom";
import { hStaticComponent, setRecordAttribute, type HtmlElement } from "@rezejs/dom/internal/html";

import { routerBase, type HistoryEntry, type RouterHistory } from "../history";
import { compileRoutes, joinRoutePaths, matchPath, routeEntryId } from "../match";
import { commit, initRouterState, pathnameOf, settleEntry, type RouterState } from "../navigation";
import { createSettledRouter } from "../router";
import { installLinkTarget } from "../target";
import type { PageMetadata, Params, RouteDefinition } from "../types";

export type SsgParamKind = "required" | "optional" | "splat";

export interface SsgParamSpec {
  readonly name: string;
  readonly kind: SsgParamKind;
}

export interface SsgRouteDescriptor {
  readonly id: string;
  readonly pattern: string;
  readonly fullPath: string;
  readonly dynamics: readonly SsgParamSpec[];
}

export type SsgStaticParams = Readonly<Record<string, string | readonly string[] | undefined>>;

export interface SsgEnumeratedUrl {
  readonly url: string;
  readonly file: string;
  readonly leafId: string;
  readonly params: Readonly<Record<string, string | readonly string[]>>;
}

export interface SsgRedirectResult {
  readonly status: "redirect";
  readonly to: string;
  readonly replace: boolean;
}

export interface SsgShellProps {
  readonly children: JSX.Element;
}

export interface SsgRouteHandle {
  /** @internal The prepared state, for post-mount imperative-redirect polling. */
  readonly state: RouterState;
}

export interface SsgRenderResult {
  readonly status: "render";
  readonly component: (props: { root?: (props: SsgShellProps) => JSX.Element }) => JSX.Element;
  readonly matches: readonly {
    readonly id: string;
    readonly params: Params;
    readonly hasData: boolean;
    readonly data?: unknown;
  }[];
  readonly metadata: PageMetadata;
  readonly handle: SsgRouteHandle;
}

export interface SsgNotFoundResult {
  readonly status: "not-found";
}

export type SsgPreparedRoute = SsgRenderResult | SsgRedirectResult | SsgNotFoundResult;

export interface SsgPrepareOptions<C = unknown> {
  /** Served base the router paths exclude; default `""`. */
  readonly base?: string;
  /** Values made available to matched route callbacks. */
  readonly context?: C;
}

function dynamicsOf(pattern: string): SsgParamSpec[] {
  const specs: SsgParamSpec[] = [];
  for (const segment of pattern.split("/")) {
    if (segment === "") continue;
    const head = segment[0]!;
    if (head === ":") {
      if (segment.endsWith("?")) specs.push({ name: segment.slice(1, -1), kind: "optional" });
      else specs.push({ name: segment.slice(1), kind: "required" });
    } else if (head === "*") {
      specs.push({ name: segment.length === 1 ? "*" : segment.slice(1), kind: "splat" });
    }
  }
  return specs;
}

/**
 * JSON-serializable leaf descriptors in table definition order. Runs the real `compileRoutes` first so invalid paths
 * throw here, then walks the table with the matcher's own join/id rules; a trailing slash from an index child is
 * normalized away so the pattern doubles as the exact `ssg.paths` key. Throws on duplicate leaf patterns.
 */
export function describeSsgRoutes(routes: readonly RouteDefinition[]): SsgRouteDescriptor[] {
  compileRoutes(routes);
  const out: SsgRouteDescriptor[] = [];
  const patternOwner = new Map<string, string>();
  const walk = (list: readonly RouteDefinition[], parentPath: string, parentId: string): void => {
    list.forEach((def, index) => {
      const joined = joinRoutePaths(parentPath, def.path);
      const pattern = joined.length > 1 ? joined.replace(/\/+$/, "") : joined;
      const id = routeEntryId(def, index, parentId);
      if (def.children !== undefined && def.children.length > 0) {
        walk(def.children, joined, id);
        return;
      }
      const owner = patternOwner.get(pattern);
      if (owner !== undefined && owner !== id) {
        throw new Error(`[reze-router] SSG leaves "${owner}" and "${id}" share pattern "${pattern}"`);
      }
      patternOwner.set(pattern, id);
      out.push({ id, pattern, fullPath: pattern, dynamics: dynamicsOf(pattern) });
    });
  };
  walk(routes, "", "");
  return out;
}

const ReservedSegments = new Set([
  "con",
  "prn",
  "aux",
  "nul",
  "com1",
  "com2",
  "com3",
  "com4",
  "com5",
  "com6",
  "com7",
  "com8",
  "com9",
  "lpt1",
  "lpt2",
  "lpt3",
  "lpt4",
  "lpt5",
  "lpt6",
  "lpt7",
  "lpt8",
  "lpt9",
]);

function decodeParam(value: string, what: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    throw new Error(`[reze-router] ${what} has malformed percent-encoding`);
  }
}

function checkControlChars(decoded: string, what: string): void {
  for (let index = 0; index < decoded.length; index++) {
    const code = decoded.charCodeAt(index);
    if (code <= 0x1f || code === 0x7f) throw new Error(`[reze-router] ${what} must not contain control characters`);
  }
}

function checkParamSegment(value: string, what: string): string {
  if (value === "") throw new Error(`[reze-router] ${what} must not be empty`);
  const decoded = decodeParam(value, what);
  if (decoded.includes("/") || decoded.includes("\\")) {
    throw new Error(`[reze-router] ${what} must not contain a slash or backslash`);
  }
  checkControlChars(decoded, what);
  if (decoded === "." || decoded === ".." || ReservedSegments.has(decoded.toLowerCase())) {
    throw new Error(`[reze-router] ${what} ${JSON.stringify(value)} is a reserved path segment`);
  }
  return encodeURIComponent(decoded);
}

function printableParam(value: unknown, what: string): string {
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return String(value);
  throw new Error(`[reze-router] ${what} must be a string, number or boolean`);
}

function buildSsgPath(
  descriptor: SsgRouteDescriptor,
  params: SsgStaticParams,
): { pathname: string; values: Record<string, string | readonly string[]> } {
  const parts: string[] = [];
  const values: Record<string, string | readonly string[]> = {};
  for (const segment of descriptor.pattern.split("/")) {
    if (segment === "") continue;
    const head = segment[0]!;
    if (head !== ":" && head !== "*") {
      parts.push(segment);
      continue;
    }
    const isOptional = head === ":" && segment.endsWith("?");
    const name = head === "*" ? (segment.length === 1 ? "*" : segment.slice(1)) : segment.slice(1, isOptional ? -1 : undefined);
    const what = `ssg.paths[${JSON.stringify(descriptor.pattern)}] param ${JSON.stringify(name)}`;
    const value = params[name];
    if (value === undefined) {
      if (isOptional) continue;
      throw new Error(`[reze-router] ssg.paths[${JSON.stringify(descriptor.pattern)}] is missing required param ${JSON.stringify(name)}`);
    }
    if (head === "*") {
      const items = typeof value === "string" ? value.split("/") : Array.isArray(value) ? value : undefined;
      if (items === undefined) {
        throw new Error(`[reze-router] ${what} takes a string or an array of strings`);
      }
      values[name] = items.map((item) => printableParam(item, `${what} part`));
      parts.push(...(values[name] as readonly string[]).map((item) => checkParamSegment(item, `${what} part`)));
      continue;
    }
    if (Array.isArray(value)) {
      throw new Error(`[reze-router] ${what} must be a string, number or boolean`);
    }
    const text = printableParam(value, what);
    values[name] = text;
    parts.push(checkParamSegment(text, what));
  }
  return { pathname: `/${parts.join("/")}`, values };
}

function canonicalUrl(pathname: string, trailingSlash: "always" | "never"): string {
  if (pathname === "/") return "/";
  return trailingSlash === "always" ? `${pathname}/` : pathname;
}

function outputFile(pathname: string): string {
  return pathname === "/" ? "index.html" : `${pathname.slice(1)}/index.html`;
}

export function enumerateSsgUrls(
  descriptors: readonly SsgRouteDescriptor[],
  paths: Readonly<Record<string, readonly SsgStaticParams[]>>,
  opts: { trailingSlash: "always" | "never" },
): SsgEnumeratedUrl[] {
  const byPattern = new Map(descriptors.map((descriptor) => [descriptor.pattern, descriptor]));
  for (const key of Object.keys(paths)) {
    const descriptor = byPattern.get(key);
    if (descriptor === undefined) {
      const known = descriptors.map((candidate) => candidate.pattern).sort();
      throw new Error(
        `[reze-router] ssg.paths has unknown route pattern ${JSON.stringify(key)}; expected one of ${known.map((pattern) => JSON.stringify(pattern)).join(", ") || "none"}`,
      );
    }
    if (descriptor.dynamics.length === 0) {
      throw new Error(
        `[reze-router] ssg.paths has an entry for static route ${JSON.stringify(key)}; remove it, static leaves enumerate once`,
      );
    }
  }
  const out: SsgEnumeratedUrl[] = [];
  const files = new Map<string, string>();
  const folded = new Map<string, string>();
  const push = (pathname: string, leafId: string, values: Record<string, string | readonly string[]>): void => {
    const file = outputFile(pathname);
    const clash = files.get(file);
    if (clash !== undefined) {
      throw new Error(
        `[reze-router] ssg.paths produces duplicate output ${JSON.stringify(file)} for ${JSON.stringify(clash)} and ${JSON.stringify(pathname)}`,
      );
    }
    files.set(file, pathname);
    const key = pathname
      .split("/")
      .map((segment) => decodeParam(segment, `ssg.paths output ${JSON.stringify(pathname)}`))
      .join("/");
    const other = folded.get(key);
    if (other !== undefined && other !== pathname) {
      throw new Error(
        `[reze-router] ssg.paths produces colliding URLs ${JSON.stringify(other)} and ${JSON.stringify(pathname)} after decoding`,
      );
    }
    folded.set(key, pathname);
    out.push({ url: canonicalUrl(pathname, opts.trailingSlash), file, leafId, params: values });
  };
  for (const descriptor of descriptors) {
    if (descriptor.dynamics.length === 0) {
      push(descriptor.pattern, descriptor.id, {});
      continue;
    }
    const sets = paths[descriptor.pattern];
    if (sets === undefined) {
      throw new Error(
        `[reze-router] ssg.paths is missing dynamic route ${JSON.stringify(descriptor.pattern)}; map it to [] to skip the leaf`,
      );
    }
    for (const params of sets) {
      for (const key of Object.keys(params)) {
        if (!descriptor.dynamics.some((spec) => spec.name === key)) {
          throw new Error(`[reze-router] ssg.paths[${JSON.stringify(descriptor.pattern)}] has unknown param ${JSON.stringify(key)}`);
        }
      }
      const built = buildSsgPath(descriptor, params);
      if (matchPath(descriptor.pattern, built.pathname) === undefined) {
        throw new Error(
          `[reze-router] ssg.paths[${JSON.stringify(descriptor.pattern)}] produces ${JSON.stringify(built.pathname)}, which does not match its own leaf`,
        );
      }
      push(built.pathname, descriptor.id, built.values);
    }
  }
  if (out.length === 0) throw new Error("[reze-router] ssg.paths enumerates no pages; add a static leaf or a dynamic entry");
  return out;
}

function createCaptureHistory(path: string, base: string): RouterHistory {
  const prefix = routerBase(base);
  const entry: HistoryEntry = { path, state: undefined, index: 0 };
  return {
    get: () => entry,
    push: () => {},
    replace: () => {},
    go: () => {},
    listen: () => () => {},
    resolve: (url) => {
      if (prefix !== "" && url.pathname !== prefix && !url.pathname.startsWith(`${prefix}/`)) return undefined;
      const pathname = prefix === "" ? url.pathname : url.pathname.slice(prefix.length) || "/";
      return pathname + url.search + url.hash;
    },
    base: prefix,
    scroll: false,
  };
}

let linkTargetInstalled = false;

/**
 * Installs the record adapter for compiler-claimed `<a>`. Idempotent and process-wide: the server entry calls this
 * before importing the app, because module-level JSX executes the ordinary `link` helper during evaluation, before any
 * `prepareServerRoute` runs. `prepareServerRoute` reinstalls it as belt-and-braces.
 */
export function installServerLinkTarget(): void {
  if (linkTargetInstalled) return;
  linkTargetInstalled = true;
  installLinkTarget({
    setAttribute: (node, name, value) => setRecordAttribute(node as HtmlElement, name, value),
    getAttribute: (node, name) => (node as HtmlElement).attributes.get(name)?.value ?? null,
  });
}

/** Removes the record adapter; processes reusing one module graph across HTML and page-DOM sessions call this between them. */
export function uninstallServerLinkTarget(): void {
  linkTargetInstalled = false;
  installLinkTarget(undefined);
}

/**
 * Prepares one router path entirely off-DOM: same matcher, IDs, params, settled preloads, metadata merge and redirect
 * rules as the browser router. Call inside the page scope; mount the returned component inside
 * `session.run(hMount(...))`. The adapter records `matches` through its own session hook after preparation and before
 * mounting, then mounts, settles, and polls `takeServerRedirect(handle)` for imperative navigation during the view.
 */
export function prepareServerRoute<C = unknown>(
  routes: readonly RouteDefinition<C>[],
  path: string,
  opts?: SsgPrepareOptions<C>,
): Promise<SsgPreparedRoute> {
  const branches = compileRoutes(routes);
  const history = createCaptureHistory(path, opts?.base ?? "");
  const state = initRouterState({ history, branches, env: "html", context: opts?.context });
  installServerLinkTarget();
  return settleEntry(state, history.get(), "initial").then((outcome) => {
    if (outcome.kind === "redirect") return { status: "redirect", to: outcome.to, replace: outcome.replace };
    if (outcome.kind === "aborted") throw new Error("[reze-router] SSG preparation was superseded while settling");
    if (outcome.matches.length === 0) return { status: "not-found" };
    const entry = state.target!;
    const location = state.targetLocation!;
    commit(state, entry, location, outcome.matches, undefined, "none");
    return {
      status: "render",
      component: hStaticComponent(createSettledRouter(state)),
      matches: outcome.matches.map((match) => ({
        id: match.route.id,
        params: match.params,
        ...(match.hasData ? { hasData: true as const, data: match.data } : { hasData: false as const }),
      })),
      metadata: outcome.metadata ?? {},
      handle: { state },
    } satisfies SsgRenderResult;
  });
}

export function takeServerRedirect(handle: SsgRouteHandle): { to: string; replace: boolean } | undefined {
  const captured = handle.state.redirectCaptured;
  handle.state.redirectCaptured = undefined;
  return captured === undefined ? undefined : { to: captured.to, replace: captured.replace };
}

export function normalizeSsgPath(pathname: string): string {
  if (pathname === "" || pathname[0] !== "/") {
    throw new Error(`[reze-router] SSG path ${JSON.stringify(pathname)} must be an absolute router path`);
  }
  if (pathname.includes("?") || pathname.includes("#")) {
    throw new Error(`[reze-router] SSG path ${JSON.stringify(pathname)} excludes query and hash; the page identity is the pathname`);
  }
  const checked: string[] = [];
  for (const segment of pathname.split("/")) {
    if (segment === "") continue;
    const what = `SSG path ${JSON.stringify(pathname)} segment`;
    const decoded = decodeParam(segment, what);
    if (decoded.includes("/") || decoded.includes("\\")) {
      throw new Error(`[reze-router] ${what} contains an encoded slash or backslash`);
    }
    checkControlChars(decoded, what);
    if (decoded === "." || decoded === ".." || ReservedSegments.has(decoded.toLowerCase())) {
      throw new Error(`[reze-router] ${what} ${JSON.stringify(segment)} is reserved`);
    }
    checked.push(segment);
  }
  return `/${checked.join("/")}`;
}

const SchemePattern = /^[a-z][a-z\d+.-]*:/i;

function schemeOf(to: string): string | undefined {
  if (to.startsWith("//")) return "//";
  return SchemePattern.exec(to)?.[0].toLowerCase();
}

export function isExternalRedirectTarget(to: string): boolean {
  const scheme = schemeOf(to);
  return scheme === "//" || scheme === "http:" || scheme === "https:";
}

export function assertRedirectTarget(to: string): void {
  if (to === "") return;
  const scheme = schemeOf(to);
  if (scheme !== undefined && scheme !== "//" && scheme !== "http:" && scheme !== "https:") {
    throw new Error(`[reze-router] redirect target ${JSON.stringify(to)} uses unsupported scheme ${JSON.stringify(scheme)}`);
  }
}

export function resolveSsgRedirectTarget(fromPathname: string, to: string): string {
  if (schemeOf(to) !== undefined) return to;
  try {
    const url = new URL(to, `http://router.invalid${fromPathname}`);
    return url.pathname + url.search + url.hash;
  } catch {
    throw new Error(`[reze-router] redirect target ${JSON.stringify(to)} from ${JSON.stringify(fromPathname)} is not a valid path`);
  }
}

/**
 * Follows single-step `prepareServerRoute` redirects to a renderable page or an external target, with visited-URL cycle
 * detection. Only the final render result's matches are recorded by the adapter; intermediate redirecting prepares
 * never reach the session.
 */
export function resolveSsgRedirectChain<C = unknown>(
  routes: readonly RouteDefinition<C>[],
  startPath: string,
  maxDepth = 10,
  opts?: SsgPrepareOptions<C>,
): Promise<SsgPreparedRoute | { readonly status: "external"; readonly to: string }> {
  const visited: string[] = [];
  let path = startPath;
  const follow = (): Promise<SsgPreparedRoute | { readonly status: "external"; readonly to: string }> => {
    const pathname = normalizeSsgPath(pathnameOf(path));
    if (visited.includes(pathname)) {
      throw new Error(`[reze-router] redirect cycle detected: ${[...visited, pathname].join(" -> ")}`);
    }
    visited.push(pathname);
    if (visited.length > maxDepth) {
      throw new Error(`[reze-router] redirect chain exceeds ${maxDepth}: ${visited.join(" -> ")}`);
    }
    return prepareServerRoute(routes, path, opts).then((prepared) => {
      if (prepared.status !== "redirect") return prepared;
      assertRedirectTarget(prepared.to);
      if (isExternalRedirectTarget(prepared.to)) return { status: "external", to: prepared.to };
      path = resolveSsgRedirectTarget(pathname, prepared.to);
      return follow();
    });
  };
  return follow();
}
