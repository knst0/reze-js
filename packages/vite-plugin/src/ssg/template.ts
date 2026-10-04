import { defaultTreeAdapter, html as htmlNames, parse, parseFragment, serialize } from "parse5";
import type { DefaultTreeAdapterMap } from "parse5";
import { joinBase, pageDepth } from "./urls";

type HtmlElement = DefaultTreeAdapterMap["element"];
type HtmlParent = DefaultTreeAdapterMap["parentNode"];

export interface TemplateHead {
  title?: string;
  description?: string;
  canonical?: string;
  robots?: string;
}

interface PageTemplate {
  templateHtml: string;
  templateFile: string;
  baseline: TemplateHead;
  rootId: string;
  base: string;
  pathname: string;
}

interface HeadAssets {
  css: readonly string[];
  js: readonly string[];
}

function* elements(parent: HtmlParent): Generator<HtmlElement> {
  for (const child of parent.childNodes) {
    if (!("tagName" in child)) continue;
    yield child;
    yield* elements(child);
  }
}

function attribute(element: HtmlElement, name: string): string | undefined {
  return element.attrs.find(attribute => attribute.name === name)?.value;
}

function setAttribute(element: HtmlElement, name: string, value: string): void {
  const existing = element.attrs.find(attribute => attribute.name === name);
  if (existing === undefined) element.attrs.push({ name, value });
  else existing.value = value;
}

function element(tag: string, attributes: Record<string, string> = {}, text?: string): HtmlElement {
  const node = defaultTreeAdapter.createElement(tag, htmlNames.NS.HTML,
    Object.entries(attributes).map(([name, value]) => ({ name, value })));
  if (text !== undefined) defaultTreeAdapter.insertText(node, text);
  return node;
}

function templateTree(source: string) {
  const document = parse(source);
  const root = document.childNodes.find(node => "tagName" in node)! as HtmlElement;
  const head = root.childNodes.find(node => node.nodeName === "head")! as HtmlElement;
  const body = root.childNodes.find(node => node.nodeName === "body")! as HtmlElement;
  return { document, head, body };
}

function headDefaults(head: HtmlElement): TemplateHead {
  const result: TemplateHead = {};
  for (const child of head.childNodes) {
    if (!("tagName" in child)) continue;
    if (child.tagName === "title" && result.title === undefined) {
      const title = child.childNodes.filter(node => node.nodeName === "#text")
        .map(node => (node as DefaultTreeAdapterMap["textNode"]).value).join("").trim();
      if (title !== "") result.title = title;
    } else if (child.tagName === "meta") {
      const name = attribute(child, "name")?.toLowerCase();
      const content = attribute(child, "content");
      if ((name === "description" || name === "robots") && content !== undefined) result[name] ??= content;
    } else if (child.tagName === "link" && attribute(child, "rel")?.toLowerCase() === "canonical") {
      const href = attribute(child, "href");
      if (href !== undefined) result.canonical ??= href;
    }
  }
  return result;
}

export function readHeadDefaults(source: string): TemplateHead {
  return headDefaults(templateTree(source).head);
}

function isModuleScript(node: HtmlElement): boolean {
  return node.tagName === "script" && attribute(node, "type")?.trim().toLowerCase() === "module";
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

function applyHead(head: HtmlElement, baseline: TemplateHead, metadata: TemplateHead): void {
  for (const name of ["title", "description", "canonical", "robots"] as const) {
    const value = metadata[name] ?? baseline[name];
    const tag = name === "title" ? "title" : name === "canonical" ? "link" : "meta";
    let target: HtmlElement | undefined;
    for (const child of [...head.childNodes]) {
      if (!("tagName" in child) || child.tagName !== tag) continue;
      if (tag === "meta" && attribute(child, "name")?.toLowerCase() !== name) continue;
      if (tag === "link" && attribute(child, "rel")?.toLowerCase() !== "canonical") continue;
      if (value === undefined || target !== undefined) defaultTreeAdapter.detachNode(child);
      else target = child;
    }
    if (value === undefined) continue;
    if (target === undefined) {
      target = element(tag);
      defaultTreeAdapter.appendChild(head, target);
    }
    if (name === "title") {
      target.childNodes.length = 0;
      defaultTreeAdapter.insertText(target, value);
    } else if (name === "canonical") {
      setAttribute(target, "rel", "canonical");
      setAttribute(target, "href", value);
    } else {
      setAttribute(target, "name", name);
      setAttribute(target, "content", value);
    }
  }
}

function appendHtml(parent: HtmlElement, source: string): void {
  if (source === "") return;
  const fragment = parseFragment(parent, source, {});
  for (const child of fragment.childNodes) defaultTreeAdapter.appendChild(parent, child);
}

function mountRoot(document: HtmlParent, rootId: string): HtmlElement {
  const matches = [...elements(document)].filter(node => attribute(node, "id") === rootId);
  if (matches.length !== 1) throw new Error(`[reze] template must contain exactly one mount element ${JSON.stringify(rootId)}`);
  return matches[0]!;
}

function rebaseAssets(document: HtmlParent, options: PageTemplate): void {
  if (options.base !== "" && options.base !== "./") return;
  const sourceUrl = new URL(options.templateFile, "https://reze.invalid/");
  const depth = pageDepth(options.pathname);
  for (const node of elements(document)) {
    for (const attr of node.attrs) {
      const isAsset = attr.name === "src" || attr.name === "poster"
        || (node.tagName === "object" && attr.name === "data")
        || (node.tagName === "link" && attr.name === "href" && attribute(node, "rel")?.toLowerCase() !== "canonical");
      if (!isAsset || /^(?:[a-zA-Z][a-zA-Z0-9+.-]*:|[/#])/.test(attr.value)) continue;
      const url = new URL(attr.value, sourceUrl);
      attr.value = joinBase(options.base, url.pathname.slice(1), depth) + url.search + url.hash;
    }
  }
}

function appendAssets(head: HtmlElement, assets: HeadAssets): void {
  const present = new Set<string>();
  for (const node of head.childNodes) {
    if (!("tagName" in node) || node.tagName !== "link") continue;
    present.add(`${attribute(node, "rel")?.toLowerCase()}\0${attribute(node, "href")}`);
  }
  for (const [rel, hrefs] of [["stylesheet", assets.css], ["modulepreload", assets.js]] as const) {
    for (const href of hrefs) {
      const key = `${rel}\0${href}`;
      if (present.has(key)) continue;
      present.add(key);
      defaultTreeAdapter.appendChild(head, element("link", { rel, href }));
    }
  }
}

const ScriptEscapes: Record<string, string> = {
  "<": "\\u003c", ">": "\\u003e", "&": "\\u0026", "\u2028": "\\u2028", "\u2029": "\\u2029",
};

function escapeScriptCharacter(character: string): string {
  return ScriptEscapes[character]!;
}

function jsonScript(attributeName: string, rootId: string, payload: string): HtmlElement {
  return element("script", { type: "application/json", [attributeName]: rootId },
    payload.replace(/[<>&\u2028\u2029]/g, escapeScriptCharacter));
}

export function buildPage(options: PageTemplate & {
  metadata: TemplateHead;
  content: string;
  payload: string;
  portals: readonly { placement: string; token: string; html: string }[];
  assets: HeadAssets;
}): string {
  const { document, head, body } = templateTree(options.templateHtml);
  rebaseAssets(document, options);
  applyHead(head, options.baseline, options.metadata);
  const root = mountRoot(document, options.rootId);
  root.childNodes.length = 0;
  appendHtml(root, options.content);
  for (const portal of options.portals) {
    if (portal.placement === "body") appendHtml(body, portal.html);
    else {
      const transport = element("template", { "data-reze-portal": portal.token });
      defaultTreeAdapter.setTemplateContent(transport as DefaultTreeAdapterMap["template"], parseFragment(transport, portal.html, {}));
      defaultTreeAdapter.appendChild(body, transport);
    }
  }
  defaultTreeAdapter.appendChild(body, jsonScript("data-reze-state", options.rootId, options.payload));
  appendAssets(head, options.assets);
  return serialize(document);
}

export function buildRedirectPage(options: PageTemplate & {
  canonical: string;
  to: string;
  replace: boolean;
  redirectSrc: string;
}): string {
  const { document, head, body } = templateTree(options.templateHtml);
  rebaseAssets(document, options);
  applyHead(head, options.baseline, { canonical: options.canonical });
  for (const node of [...elements(document)]) {
    if (isModuleScript(node) || (node.tagName === "script" && attribute(node, "data-reze-state") !== undefined)) {
      defaultTreeAdapter.detachNode(node);
    }
  }
  const root = mountRoot(document, options.rootId);
  root.childNodes.length = 0;
  const paragraph = element("p");
  defaultTreeAdapter.appendChild(paragraph, element("a", { href: options.to }, `Continue to ${options.to}`));
  defaultTreeAdapter.appendChild(root, paragraph);
  defaultTreeAdapter.appendChild(head, element("meta", { "http-equiv": "refresh", content: `0;url=${options.to}` }));
  defaultTreeAdapter.appendChild(body, jsonScript("data-reze-redirect", options.rootId,
    JSON.stringify({ to: options.to, replace: options.replace })));
  defaultTreeAdapter.appendChild(body, element("script", { type: "module", src: options.redirectSrc }));
  return serialize(document);
}
