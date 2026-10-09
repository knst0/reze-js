import { VoidElements } from "../../../../crates/reze_compiler/src/html-data.json";

/**
 * Namespace key carried by HTML records: the empty string for HTML,
 * `"svg"` for SVG and `"math"` for MathML. Matches the compiler HTML
 * backend's namespace argument.
 */
export type HtmlNamespaceKey = "" | "svg" | "math";

/** Discriminant shared by every HTML record node. */
export type HtmlNodeKind = "element" | "text" | "marker" | "range";

export type HtmlNode = HtmlElement | HtmlText | HtmlMarker | HtmlRange;
export type HtmlParent = HtmlElement | HtmlRange;

/** Optional compiler association attached by the parent layer. */
export interface HtmlMeta {
  staticIndex?: number;
  site?: unknown;
  token?: string;
}

export interface HtmlStoredAttribute {
  name: string;
  ns: string | undefined;
  bool: boolean;
  value: string | undefined;
}

export type HtmlClassState =
  | { readonly mode: "none" }
  | { readonly mode: "string"; readonly value: string }
  | { readonly mode: "tokens"; tokens: Set<string> };

export type HtmlStyleState =
  | { readonly mode: "none" }
  | { readonly mode: "text"; readonly cssText: string }
  | { readonly mode: "map"; properties: Map<string, string> };

export interface HtmlElement {
  readonly kind: "element";
  tag: string;
  ns: HtmlNamespaceKey;
  attributes: Map<string, HtmlStoredAttribute>;
  classState: HtmlClassState;
  styleState: HtmlStyleState;
  value: string | undefined;
  hasValue: boolean;
  checked: boolean;
  selected: boolean;
  selectValue: string | undefined;
  hasSelectValue: boolean;
  innerHTML: string | undefined;
  genericProps: Map<string, unknown>;
  spreadPrev: Map<string, unknown>;
  children: HtmlNode[];
  parent: HtmlParent | undefined;
  meta: HtmlMeta;
}

export interface HtmlText {
  readonly kind: "text";
  data: string;
  parent: HtmlParent | undefined;
  meta: HtmlMeta;
}

export interface HtmlMarker {
  readonly kind: "marker";
  parent: HtmlParent | undefined;
  meta: HtmlMeta;
}

export interface HtmlRange {
  readonly kind: "range";
  token: string;
  children: HtmlNode[];
  parent: HtmlParent | undefined;
  meta: HtmlMeta;
  marked: boolean;
  wire: string | undefined;
  sink: DirtySink | undefined;
}

export interface DirtySink {
  add(range: HtmlRange): void;
}

/** Source-aware failure for unsupported values and unrepresentable layouts. */
export class HtmlRecordError extends Error {
  readonly site: unknown;
  constructor(detail: string, context?: string, site?: unknown) {
    super(context === undefined ? detail : `${detail} (${context})`);
    this.name = "HtmlRecordError";
    this.site = site;
  }
}

/** Best-effort one-line rendering of a compiler site reference. */
export function formatSite(site: unknown): string | undefined {
  if (site === null || site === undefined) {
    return undefined;
  }
  if (typeof site === "string") {
    return site;
  }
  if (typeof site === "object") {
    const record = site as Record<string, unknown>;
    const key = record["key"];
    const module = record["module"];
    const ordinal = record["ordinal"];
    if (typeof key === "string" && typeof module === "string" && typeof ordinal === "number") {
      return `${module}#${key}_${ordinal.toString(36)}`;
    }
  }
  return undefined;
}

const RawtextTags: Record<string, true> = {
  script: true,
  style: true,
  iframe: true,
  noembed: true,
  noframes: true,
  xmp: true,
  plaintext: true,
};

const RcdataTags: Record<string, true> = { textarea: true, title: true };

const LfStripTags: Record<string, true> = { pre: true, listing: true, textarea: true };

const TagPattern = /^[A-Za-z][A-Za-z0-9._:-]*$/;

export function isVoidTag(tag: string, ns: HtmlNamespaceKey): boolean {
  return ns === "" && Object.hasOwn(VoidElements, tag.toLowerCase());
}

/** Rawtext containers whose content parses as verbatim text until the end tag. */
export function isRawtextTag(tag: string, ns: HtmlNamespaceKey): boolean {
  return ns === "" && Object.hasOwn(RawtextTags, tag.toLowerCase());
}

/** RCDATA containers whose text is escaped but never hosts markers or elements. */
export function isRcdataTag(tag: string, ns: HtmlNamespaceKey): boolean {
  return ns === "" && Object.hasOwn(RcdataTags, tag.toLowerCase());
}

/** Elements where the parser strips one leading newline from their content. */
export function isLfStripTag(tag: string, ns: HtmlNamespaceKey): boolean {
  return ns === "" && Object.hasOwn(LfStripTags, tag.toLowerCase());
}

function checkTag(tag: string, site?: unknown): void {
  if (!TagPattern.test(tag)) {
    throw new HtmlRecordError(`Invalid element tag "${tag}"; expected a tag name with optional namespace prefix`, formatSite(site), site);
  }
}

function checkNamespace(ns: HtmlNamespaceKey, site?: unknown): void {
  if (ns !== "" && ns !== "svg" && ns !== "math") {
    throw new HtmlRecordError(`Invalid namespace key "${ns as string}"; expected "", "svg" or "math"`, formatSite(site), site);
  }
}

/** Native element record; static attributes and children attach afterwards. */
export function createElement(tag: string, ns: HtmlNamespaceKey = "", meta?: HtmlMeta): HtmlElement {
  checkTag(tag, meta?.site);
  checkNamespace(ns, meta?.site);
  return {
    kind: "element",
    tag: ns === "" ? tag.toLowerCase() : tag,
    ns,
    attributes: new Map(),
    classState: { mode: "none" },
    styleState: { mode: "none" },
    value: undefined,
    hasValue: false,
    checked: false,
    selected: false,
    selectValue: undefined,
    hasSelectValue: false,
    innerHTML: undefined,
    genericProps: new Map(),
    spreadPrev: new Map(),
    children: [],
    parent: undefined,
    meta: meta ?? {},
  };
}

/** Mutable text record; dynamic emptiness is the empty string. */
export function createText(data = "", meta?: HtmlMeta): HtmlText {
  return { kind: "text", data, parent: undefined, meta: meta ?? {} };
}

/** Single anchor record serializing as `<!>` outside rawtext/RCDATA. */
export function createMarker(meta?: HtmlMeta): HtmlMarker {
  return { kind: "marker", parent: undefined, meta: meta ?? {} };
}

/** Range record; serializes as a comment pair only when `marked`. */
export function createRange(token: string, meta?: HtmlMeta): HtmlRange {
  return { kind: "range", token, children: [], parent: undefined, meta: meta ?? {}, marked: false, wire: undefined, sink: undefined };
}

let trackedSinks = 0;

export function trackSink(delta: 1 | -1): void {
  trackedSinks += delta;
}

export function markDirty(node: HtmlParent | HtmlText): void {
  if (trackedSinks === 0) return;
  let target: HtmlRange | undefined;
  let current: HtmlNode = node;
  for (;;) {
    if (target === undefined && current.kind === "range" && current.wire !== undefined) target = current;
    const parent: HtmlParent | undefined = current.parent;
    if (parent === undefined) break;
    current = parent;
  }
  if (target !== undefined && current.kind === "range") current.sink?.add(target);
}

function describeParent(parent: HtmlParent): string {
  return parent.kind === "element" ? `<${parent.tag}>` : `range "${parent.token}"`;
}

function checkPlaceable(parent: HtmlParent, child: HtmlNode, site?: unknown): void {
  if (child.parent !== undefined) {
    throw new HtmlRecordError(
      "Cannot attach a node that already has a parent; remove it first",
      `${describeParent(parent)} <- ${child.kind}`,
      site,
    );
  }
  if (parent.kind === "range") {
    return;
  }
  if (parent.innerHTML !== undefined) {
    throw new HtmlRecordError(
      `Cannot attach children to <${parent.tag}> with opaque innerHTML; use textContent to replace it`,
      formatSite(site) ?? describeParent(parent),
      site,
    );
  }
  if (isVoidTag(parent.tag, parent.ns)) {
    throw new HtmlRecordError(
      `Cannot attach children to void <${parent.tag}>; the parser would keep them as siblings`,
      formatSite(site) ?? describeParent(parent),
      site,
    );
  }
  if (parent.ns === "" && parent.tag.toLowerCase() === "plaintext") {
    if (child.kind !== "marker") {
      throw new HtmlRecordError(
        "Cannot place content inside <plaintext>; it swallows the rest of the document as text",
        "render the text in a normal element instead",
        site,
      );
    }
    return;
  }
  if (isRawtextTag(parent.tag, parent.ns) || isRcdataTag(parent.tag, parent.ns)) {
    if (child.kind === "text" || child.kind === "marker" || child.kind === "range") {
      return;
    }
    const detail = `<${child.tag}>`;
    throw new HtmlRecordError(
      `Cannot place ${detail} inside <${parent.tag}>; the parser reads its content as text`,
      formatSite(site) ?? describeParent(parent),
      site,
    );
  }
}

/** Appends `child` to `parent` preserving order and record linkage. */
export function attachChild(parent: HtmlParent, child: HtmlNode, site?: unknown): void {
  checkPlaceable(parent, child, site ?? child.meta.site);
  child.parent = parent;
  parent.children.push(child);
  markDirty(parent);
}

/** Inserts `child` before the direct child `anchor`. */
export function insertBefore(parent: HtmlParent, child: HtmlNode, anchor: HtmlNode, site?: unknown): void {
  checkPlaceable(parent, child, site ?? child.meta.site);
  const index = parent.children.indexOf(anchor);
  if (index === -1 || anchor.parent !== parent) {
    throw new HtmlRecordError("Cannot insert before an anchor from another parent", `${describeParent(parent)} <- ${child.kind}`, site);
  }
  child.parent = parent;
  parent.children.splice(index, 0, child);
  markDirty(parent);
}

/** Removes a direct child and clears its linkage. */
export function removeChild(parent: HtmlParent, child: HtmlNode): void {
  const index = parent.children.indexOf(child);
  if (index === -1 || child.parent !== parent) {
    throw new HtmlRecordError(
      "Cannot remove a node that is not a child of this parent",
      `${describeParent(parent)} <- ${child.kind}`,
      child.meta.site,
    );
  }
  parent.children.splice(index, 1);
  child.parent = undefined;
  markDirty(parent);
}

/** Detaches `node` from its parent, if any. */
export function detach(node: HtmlNode): void {
  if (node.parent !== undefined) {
    removeChild(node.parent, node);
  }
}

/** Removes every child of `parent` and clears their linkage. */
export function clearChildren(parent: HtmlParent): void {
  if (parent.children.length === 0) return;
  for (const child of parent.children) {
    child.parent = undefined;
  }
  parent.children.length = 0;
  markDirty(parent);
}

/** Replaces the data of a text record. */
export function setTextData(node: HtmlText, data: string): void {
  if (node.data === data) return;
  node.data = data;
  markDirty(node);
}
