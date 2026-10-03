import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve as resolvePath } from "node:path";
export interface PrerenderOptions {
  /** CSS id selector of the shell element whose content is prerendered. Default `"#app"`. */
  selector?: string;
}

export interface PrerenderComponentRef {
  request: string | null;
  path: string[];
}

export interface PrerenderHole {
  id: number;
  target: PrerenderComponentRef;
}

export type PrerenderTree =
  | { Html: { html: string; namespace: string } }
  | { Text: string }
  | { Children: PrerenderTree[] }
  | { Mixed: { html: string; holes: PrerenderHole[] } }
  | { Component: PrerenderComponentRef }
  | "Empty";

export interface PrerenderComponent {
  name: string;
  exported: string[];
  tree: PrerenderTree;
}

export interface PrerenderModule {
  components: PrerenderComponent[];
  roots: PrerenderTree[];
}

export interface PrerenderIndex {
  getModule(id: string): PrerenderModule | undefined;
  resolve(request: string, importer: string): Promise<string | undefined>;
}

/** Mirrors `escape_text` in the compiler: `&` always, `<` in text. */
export function escapeText(value: string): string {
  let out = "";
  let start = 0;
  for (let i = 0; i < value.length; i++) {
    const replacement = value[i] === "&" ? "&amp;" : value[i] === "<" ? "&lt;" : undefined;
    if (replacement === undefined) continue;
    out += value.slice(start, i) + replacement;
    start = i + 1;
  }
  return out + value.slice(start);
}

function isResolvable(target: PrerenderComponentRef): boolean {
  return target.path.length > 0;
}
interface Resolved {
  tree: PrerenderTree;
  importer: string;
  key: string;
}

async function resolveComponent(
  target: PrerenderComponentRef,
  importer: string,
  index: PrerenderIndex,
  stack: string[],
): Promise<Resolved | undefined> {
  if (!isResolvable(target)) return undefined;
  const name = target.path[0]!;
  if (target.request === null) {
    const module = index.getModule(importer);
    const component = module?.components.find((candidate) => candidate.name === name);
    const key = `${importer}::${name}`;
    if (component === undefined || stack.includes(key)) return undefined;
    return { tree: component.tree, importer, key };
  }
  const id = await index.resolve(target.request, importer);
  if (id === undefined) return undefined;
  const module = index.getModule(id);
  const component = module?.components.find((candidate) => candidate.exported.includes(name) || candidate.name === name);
  if (component === undefined) return undefined;
  const key = `${id}::${component.name}`;
  if (stack.includes(key)) return undefined;
  return { tree: component.tree, importer: id, key };
}
async function renderResolved(resolved: Resolved, index: PrerenderIndex, stack: string[]): Promise<string> {
  return renderTree(resolved.tree, index, resolved.importer, [...stack, resolved.key]);
}

/** Composes `tree` to an HTML string, resolving component holes through `index`. Unresolvable holes and cycles render as nothing. */
export async function renderTree(tree: PrerenderTree, index: PrerenderIndex, importer: string, stack: string[] = []): Promise<string> {
  if (tree === "Empty") return "";
  if (typeof tree === "object" && "Text" in tree) return escapeText(tree.Text);
  if (typeof tree === "object" && "Html" in tree) return tree.Html.html;
  if (typeof tree === "object" && "Children" in tree) {
    let out = "";
    for (const child of tree.Children) out += await renderTree(child, index, importer, stack);
    return out;
  }
  if (typeof tree === "object" && "Mixed" in tree) {
    let out = tree.Mixed.html;
    for (const hole of tree.Mixed.holes) {
      const resolved = await resolveComponent(hole.target, importer, index, stack);
      const rendered = resolved === undefined ? "" : await renderResolved(resolved, index, stack);
      out = out.replace(`<!--reze${hole.id}-->`, () => rendered);
    }
    return out;
  }
  if (typeof tree === "object" && "Component" in tree) {
    const resolved = await resolveComponent(tree.Component, importer, index, stack);
    return resolved === undefined ? "" : renderResolved(resolved, index, stack);
  }
  return "";
}

const ShellPattern = /^#([\w-]+)$/;

/** Replaces the inner content of the `selector` element (`#id` only) with `content`. Returns `undefined` when the shell is absent. */
export function injectShell(html: string, selector: string, content: string): string | undefined {
  const id = ShellPattern.exec(selector)?.[1];
  if (id === undefined) return undefined;
  const pattern = new RegExp(`<([A-Za-z][^\\s/>]*)([^>]*\\sid=(["'])${id}\\3[^>]*)>([\\s\\S]*?)</\\1>`);
  if (!pattern.test(html)) return undefined;
  return html.replace(pattern, `<$1$2>${content}</$1>`);
}

interface PrerenderContext {
  warn(message: string): void;
}

/** Renders the entry shell of one HTML file. Returns `undefined` when there is nothing to inline. */
export async function prerenderHtml(
  html: string,
  file: string,
  rootDir: string,
  sidecars: Map<string, PrerenderModule>,
  selector: string,
  context: PrerenderContext,
): Promise<string | undefined> {
  let source: string;
  try {
    source = readFileSync(file, "utf8");
  } catch {
    return undefined;
  }
  const src = source.match(/<script[^>]*\btype="module"[^>]*\ssrc="([^"]+)"/)?.[1];
  if (src === undefined) return undefined;
  const entry = src.startsWith("/") ? rootDir + src : `${file.split("/").slice(0, -1).join("/")}/${src}`;
  const index: PrerenderIndex = {
    getModule: (id) => sidecars.get(id.replace(/[?#].*$/, "")),
    resolve: async (request, importer) => resolveRequest(request, importer),
  };
  const root = index.getModule(entry)?.roots[0];
  if (root === undefined) return undefined;
  const updated = injectShell(html, selector, await renderTree(root, index, entry));
  if (updated === undefined) {
    context.warn(`[reze] prerender: no element matches "${selector}" in ${file}`);
    return undefined;
  }
  return updated;
}

const ProbeExtensions = [".tsx", ".ts", ".jsx", ".js"];

function resolveRequest(request: string, importer: string): string | undefined {
  if (!request.startsWith(".")) return undefined;
  const base = resolvePath(dirname(importer), request);
  if (existsSync(base)) return base;
  for (const extension of ProbeExtensions) {
    if (existsSync(base + extension)) return base + extension;
    if (existsSync(`${base}/index${extension}`)) return `${base}/index${extension}`;
  }
  return undefined;
}
