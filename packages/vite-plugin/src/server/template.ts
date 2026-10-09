import { defaultTreeAdapter, parse, serialize } from "parse5";
import type { DefaultTreeAdapterMap } from "parse5";

type HtmlElement = DefaultTreeAdapterMap["element"];
type HtmlParent = DefaultTreeAdapterMap["parentNode"];

export interface TemplateHead {
  title?: string;
  description?: string;
  canonical?: string;
  robots?: string;
}

export interface TemplateParts {
  beforeHeadEnd: string;
  headEndToRoot: string;
  rootEndToBodyEnd: string;
  bodyEndToEnd: string;
}

export interface PreparedTemplate {
  parts: TemplateParts;
  headDefaults: TemplateHead;
}

const HeadSentinel = "<!--rz-head-->";
const RootSentinel = "<!--rz-root-->";
const BodySentinel = "<!--rz-body-->";

function* elements(parent: HtmlParent): Generator<HtmlElement> {
  for (const child of parent.childNodes) {
    if (!("tagName" in child)) continue;
    yield child;
    yield* elements(child);
  }
}

function attribute(element: HtmlElement, name: string): string | undefined {
  return element.attrs.find((attribute) => attribute.name === name)?.value;
}

function setAttribute(element: HtmlElement, name: string, value: string): void {
  const existing = element.attrs.find((attribute) => attribute.name === name);
  if (existing === undefined) element.attrs.push({ name, value });
  else existing.value = value;
}

function isModuleScript(node: HtmlElement): boolean {
  return node.tagName === "script" && attribute(node, "type")?.trim().toLowerCase() === "module";
}

function findRoot(document: DefaultTreeAdapterMap["document"], rootId: string): HtmlElement {
  const roots = [...elements(document)].filter((node) => attribute(node, "id") === rootId);
  if (roots.length !== 1) {
    throw new Error(`[reze] built template must contain exactly one element with id ${JSON.stringify(rootId)}`);
  }
  return roots[0]!;
}

function metadataKind(node: HtmlElement): keyof TemplateHead | undefined {
  if (node.tagName === "title") return "title";
  if (node.tagName === "meta") {
    const name = attribute(node, "name")?.toLowerCase();
    return name === "description" || name === "robots" ? name : undefined;
  }
  if (node.tagName === "link" && attribute(node, "rel")?.toLowerCase() === "canonical") return "canonical";
  return undefined;
}

function metadataValue(node: HtmlElement, kind: keyof TemplateHead): string | undefined {
  if (kind === "title") {
    const text = node.childNodes
      .filter((child) => child.nodeName === "#text")
      .map((child) => (child as DefaultTreeAdapterMap["textNode"]).value)
      .join("")
      .trim();
    return text === "" ? undefined : text;
  }
  return kind === "canonical" ? attribute(node, "href") : attribute(node, "content");
}

function takeHeadDefaults(head: HtmlElement): TemplateHead {
  const result: TemplateHead = {};
  for (const child of [...head.childNodes]) {
    if (!("tagName" in child)) continue;
    const kind = metadataKind(child);
    if (kind === undefined) continue;
    const value = metadataValue(child, kind);
    if (value !== undefined) result[kind] ??= value;
    defaultTreeAdapter.detachNode(child);
  }
  return result;
}

function splitSentinels(html: string): TemplateParts {
  const head = html.indexOf(HeadSentinel);
  const root = html.indexOf(RootSentinel);
  const body = html.indexOf(BodySentinel);
  if (head < 0 || root < head || body < root) throw new Error("[reze] built template sentinels are out of order");
  return {
    beforeHeadEnd: html.slice(0, head),
    headEndToRoot: html.slice(head + HeadSentinel.length, root),
    rootEndToBodyEnd: html.slice(root + RootSentinel.length, body),
    bodyEndToEnd: html.slice(body + BodySentinel.length),
  };
}

export function validateTemplate(source: string, file: string, rootId: string, bootstrapSrc: string): void {
  let scripts = 0;
  let roots = 0;
  for (const node of elements(parse(source))) {
    if (isModuleScript(node) && attribute(node, "src") === bootstrapSrc) scripts++;
    if (attribute(node, "id") === rootId) roots++;
  }
  if (scripts !== 1) {
    throw new Error(`[reze] template ${file} must contain exactly one <script type="module" src="${bootstrapSrc}">, found ${scripts}`);
  }
  if (roots !== 1) {
    throw new Error(`[reze] template ${file} must contain exactly one element with id ${JSON.stringify(rootId)}, found ${roots}`);
  }
}

export function bootstrapScriptSrc(source: string, bootstrapFile: string): string {
  for (const node of elements(parse(source))) {
    if (!isModuleScript(node)) continue;
    const src = attribute(node, "src") ?? "";
    if (src === bootstrapFile || src.endsWith(`/${bootstrapFile}`)) return src;
  }
  throw new Error(`[reze] built template no longer references the client bootstrap ${JSON.stringify(bootstrapFile)}`);
}

export function countRootIds(source: string, rootId: string): number {
  let roots = 0;
  for (const node of elements(parse(source))) {
    if (attribute(node, "id") === rootId) roots++;
  }
  return roots;
}

export function prepareTemplate(source: string, rootId: string): PreparedTemplate {
  const document = parse(source);
  const top = document.childNodes.find((node) => "tagName" in node) as HtmlElement | undefined;
  const head = top?.childNodes.find((node) => node.nodeName === "head") as HtmlElement | undefined;
  const body = top?.childNodes.find((node) => node.nodeName === "body") as HtmlElement | undefined;
  if (top === undefined || head === undefined || body === undefined) {
    throw new Error("[reze] built template must contain a head and a body");
  }
  const headDefaults = takeHeadDefaults(head);
  for (const node of elements(head)) {
    if (isModuleScript(node) && attribute(node, "src") !== undefined) setAttribute(node, "async", "");
  }
  const root = findRoot(document, rootId);
  for (const child of [...root.childNodes]) defaultTreeAdapter.detachNode(child);
  defaultTreeAdapter.appendChild(root, defaultTreeAdapter.createCommentNode("rz-root"));
  defaultTreeAdapter.appendChild(head, defaultTreeAdapter.createCommentNode("rz-head"));
  defaultTreeAdapter.appendChild(body, defaultTreeAdapter.createCommentNode("rz-body"));
  return { parts: splitSentinels(serialize(document)), headDefaults };
}

export function rebaseParts(parts: TemplateParts, depth: number): TemplateParts {
  const prefix = depth === 0 ? "./" : "../".repeat(depth);
  const rebase = (html: string) => html.replace(/(\s(?:src|href)=")\.\//g, (_, lead: string) => `${lead}${prefix}`);
  return {
    beforeHeadEnd: rebase(parts.beforeHeadEnd),
    headEndToRoot: rebase(parts.headEndToRoot),
    rootEndToBodyEnd: rebase(parts.rootEndToBodyEnd),
    bodyEndToEnd: parts.bodyEndToEnd,
  };
}

function escapeAttribute(value: string): string {
  return value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

const ModuleSrcScript = /<script\b[^>]*\btype="module"[^>]*\bsrc="[^"]*"[^>]*><\/script>/g;
function withoutBootstrap(html: string): string {
  return html.replace(ModuleSrcScript, "");
}

export function buildRedirectPage(options: { parts: TemplateParts; to: string; replace: boolean; redirectSrc: string }): string {
  const target = escapeAttribute(options.to);
  const payload = JSON.stringify({ to: options.to, replace: options.replace })
    .replace(/</g, "\\u003c")
    .replace(/>/g, "\\u003e")
    .replace(/&/g, "\\u0026")
    .replace(/\u2028/g, "\\u2028")
    .replace(/\u2029/g, "\\u2029");
  return [
    withoutBootstrap(options.parts.beforeHeadEnd),
    `<link rel="canonical" href="${target}">`,
    `<meta http-equiv="refresh" content="0;url=${target}">`,
    `<meta name="robots" content="noindex">`,
    withoutBootstrap(options.parts.headEndToRoot),
    `<p><a href="${target}">Continue to ${target}</a></p>`,
    withoutBootstrap(options.parts.rootEndToBodyEnd),
    `<script type="application/json" data-reze-redirect>${payload}</script>`,
    `<script type="module" src="${escapeAttribute(options.redirectSrc)}"></script>`,
    options.parts.bodyEndToEnd,
  ].join("");
}
