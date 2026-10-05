import { AttributeEscapes, TextEscapes } from "../../../../crates/reze_compiler/src/html-data.json";
import type { LayoutNode, RangeKind } from "../hydration/protocol";

import {
  type HtmlElement,
  type HtmlNode,
  type HtmlRange,
  HtmlRecordError,
  formatSite,
  isLfStripTag,
  isRawtextTag,
  isRcdataTag,
  isValidToken,
  isVoidTag,
} from "./tree";

export interface HtmlSerializeOptions {
  token?: string;
  ownerTokens?: ReadonlyMap<string, string>;
}

/** Session binding for one range record: owner identity, flow kind, portal placement. */
export interface HtmlLayoutRangeInfo {
  readonly ownerId: string;
  readonly range: RangeKind;
  readonly placement?: "body" | "inert";
}

/**
 * Session callbacks for layout-only data the records cannot carry: range
 * owner/kind/placement and site-key overrides. Element tokens, static indices
 * and default site keys come from record meta.
 */
export interface HtmlLayoutResolver {
  rangeInfo?(range: HtmlRange): HtmlLayoutRangeInfo;
  elementSite?(el: HtmlElement): string | undefined;
  rangeSite?(range: HtmlRange): string | undefined;
}

export interface HtmlLayoutOptions extends HtmlSerializeOptions {
  resolver?: HtmlLayoutResolver;
}

interface SyntheticWrapper {
  readonly kind: "synthetic";
  tag: string;
  children: EmitChild[];
}

/**
 * A range paired with its container-normalized children. The original record
 * flows through untouched, so identity-keyed session metadata resolves on
 * `source` without copies or token-keyed fallbacks.
 */
interface NormalizedRangeView {
  readonly kind: "normalized";
  source: HtmlRange;
  children: EmitChild[];
}

type EmitChild = HtmlNode | SyntheticWrapper | NormalizedRangeView;

type ContainerKind = "table" | "section" | "row" | "colgroup" | "select" | "neutral";

function escapeWith(value: string, table: Record<string, string>): string {
  let out = "";
  let start = 0;
  for (let i = 0; i < value.length; i++) {
    const replacement = table[value[i]];
    if (replacement !== undefined) {
      out += value.slice(start, i) + replacement;
      start = i + 1;
    }
  }
  return out + value.slice(start);
}

function appendAttributeValue(out: string[], value: string): void {
  if (value.length === 0) {
    return;
  }
  const needsQuotes = value.endsWith("/") || /[ \t\n\f\r"'=<>`]/.test(value);
  out.push("=");
  if (needsQuotes) {
    out.push('"', escapeWith(value, AttributeEscapes), '"');
  } else {
    out.push(escapeWith(value, AttributeEscapes));
  }
}

function hasClosingDelimiter(text: string, tag: string): boolean {
  const lower = text.toLowerCase();
  const needle = `</${tag.toLowerCase()}`;
  let index = lower.indexOf(needle);
  while (index !== -1) {
    const after = lower[index + needle.length];
    if (
      after === undefined ||
      after === "\t" ||
      after === "\n" ||
      after === "\f" ||
      after === " " ||
      after === "/" ||
      after === ">"
    ) {
      return true;
    }
    index = lower.indexOf(needle, index + 1);
  }
  return false;
}

function containerKind(el: HtmlElement): ContainerKind {
  if (el.ns !== "") {
    return "neutral";
  }
  switch (el.tag.toLowerCase()) {
    case "table":
      return "table";
    case "thead":
    case "tbody":
    case "tfoot":
      return "section";
    case "tr":
      return "row";
    case "colgroup":
      return "colgroup";
    case "select":
      return "select";
    default:
      return "neutral";
  }
}

function failLayout(detail: string, hint: string, el: HtmlElement): never {
  throw new HtmlRecordError(
    `Cannot serialize <${el.tag}> that ${detail}; ${hint}`,
    formatSite(el.meta.site),
    el.meta.site,
  );
}

const TableMembers: Record<string, true> = { tr: true, td: true, th: true };
const RowMembers: Record<string, true> = { td: true, th: true };
const TableKept: Record<string, true> = {
  caption: true,
  colgroup: true,
  thead: true,
  tbody: true,
  tfoot: true,
  style: true,
  script: true,
  template: true,
};
const SectionKept: Record<string, true> = { tr: true, style: true, script: true, template: true };
const RowKept: Record<string, true> = { style: true, script: true, template: true };
const SelectKept: Record<string, true> = {
  option: true,
  optgroup: true,
  hr: true,
  script: true,
  template: true,
};

function normalizedRange(range: HtmlRange, kind: ContainerKind): NormalizedRangeView {
  return { kind: "normalized", source: range, children: normalizeList(range.children, kind) };
}

function normalizeList(children: HtmlNode[], kind: ContainerKind): EmitChild[] {
  if (kind === "neutral") {
    if (!children.some((child) => child.kind === "range")) {
      return children;
    }
    return children.map((child) =>
      child.kind === "range" ? normalizedRange(child, kind) : child,
    );
  }
  const out: EmitChild[] = [];
  let run: EmitChild[] = [];
  let runCols = false;
  const flushRun = (): void => {
    if (run.length === 0) {
      return;
    }
    out.push({ kind: "synthetic", tag: runCols ? "colgroup" : "tbody", children: runCols ? run : subgroupCells(run) });
    run = [];
    runCols = false;
  };
  const pushGap = (node: EmitChild): void => {
    if (run.length > 0) {
      run.push(node);
    } else {
      out.push(node);
    }
  };
  for (const child of children) {
    if (child.kind === "range") {
      const normalized = normalizedRange(child, kind);
      flushRun();
      out.push(normalized);
      continue;
    }
    if (child.kind === "marker") {
      if (kind === "table" || kind === "section") {
        pushGap(child);
      } else {
        out.push(child);
      }
      continue;
    }
    if (child.kind === "text") {
      if (child.data === "" || /^[\t\n\f\r ]+$/.test(child.data)) {
        if (kind === "table" || kind === "section") {
          pushGap(child);
        } else {
          out.push(child);
        }
        continue;
      }
      flushRun();
      if (kind === "select") {
        throw new HtmlRecordError(
          `Cannot serialize text "${child.data}" directly inside <select>; the parser drops it`,
          "move the text into an option",
          undefined,
        );
      }
      throw new HtmlRecordError(
        `Cannot serialize text "${child.data}" directly inside <${kind}>; the parser foster-parents it out of the table`,
        "move the text into a table cell",
        undefined,
      );
    }
    const tag = child.tag.toLowerCase();
    if (kind === "table") {
      if (child.ns === "" && Object.hasOwn(TableMembers, tag)) {
        if (runCols) {
          flushRun();
        }
        run.push(child);
        continue;
      }
      if (child.ns === "" && tag === "col") {
        if (!runCols) {
          flushRun();
        }
        run.push(child);
        runCols = true;
        continue;
      }
      flushRun();
      if (child.ns !== "" || !Object.hasOwn(TableKept, tag)) {
        failTableChild(child);
      }
      out.push(child);
      continue;
    }
    if (kind === "section") {
      if (child.ns === "" && Object.hasOwn(RowMembers, tag)) {
        run.push(child);
        continue;
      }
      flushRun();
      if (child.ns === "" && tag === "col") {
        const parent = child.parent;
        const context = parent !== undefined && parent.kind === "element" ? parent.tag : "tbody";
        throw new HtmlRecordError(
          `Cannot serialize <col> directly inside <${context}>; the parser moves it out`,
          "wrap `<col>` in an explicit `<colgroup>`",
          child.meta.site,
        );
      }
      if (child.ns !== "" || !Object.hasOwn(SectionKept, tag)) {
        failTableChild(child);
      }
      out.push(child);
      continue;
    }
    if (kind === "row") {
      if (child.ns === "" && Object.hasOwn(RowMembers, tag)) {
        out.push(child);
        continue;
      }
      if (child.ns !== "" || !Object.hasOwn(RowKept, tag)) {
        failTableChild(child);
      }
      out.push(child);
      continue;
    }
    if (kind === "colgroup") {
      if (child.ns === "" && tag === "col") {
        out.push(child);
        continue;
      }
      if (child.ns !== "" || tag !== "template") {
        failTableChild(child);
      }
      out.push(child);
      continue;
    }
    flushRun();
    if (child.ns !== "" || !Object.hasOwn(SelectKept, tag)) {
      failTableChild(child);
    }
    out.push(child);
  }
  flushRun();
  return out;
}

function failTableChild(el: HtmlElement): never {
  const parent = el.parent;
  const context = parent !== undefined && parent.kind === "element" ? parent.tag : "table";
  throw new HtmlRecordError(
    `Cannot serialize <${el.tag}> directly inside <${context}>; the parser relocates or drops it`,
    "restructure the markup so it parses as written",
    el.meta.site,
  );
}

function subgroupCells(run: EmitChild[]): EmitChild[] {
  const out: EmitChild[] = [];
  let group: EmitChild[] = [];
  const flushGroup = (): void => {
    if (group.length > 0) {
      out.push({ kind: "synthetic", tag: "tr", children: group });
      group = [];
    }
  };
  for (const item of run) {
    if (
      item.kind === "element" &&
      item.ns === "" &&
      (item.tag.toLowerCase() === "td" || item.tag.toLowerCase() === "th")
    ) {
      group.push(item);
      continue;
    }
    if ((item.kind === "marker" || item.kind === "text") && group.length > 0) {
      group.push(item);
      continue;
    }
    flushGroup();
    out.push(item);
  }
  flushGroup();
  return out;
}

function optionText(option: HtmlElement): string {
  let out = "";
  const visit = (nodes: HtmlNode[]): void => {
    for (const node of nodes) {
      if (node.kind === "text") {
        out += node.data;
      } else if (node.kind === "element" || node.kind === "range") {
        visit(node.children);
      }
    }
  };
  visit(option.children);
  return out;
}

function collectOptions(root: HtmlElement, into: HtmlElement[]): void {
  const visit = (nodes: HtmlNode[]): void => {
    for (const node of nodes) {
      if (node.kind === "element") {
        if (node.ns === "" && node.tag.toLowerCase() === "option") {
          into.push(node);
        } else if (node.ns !== "" || node.tag.toLowerCase() !== "select") {
          visit(node.children);
        }
      } else if (node.kind === "range") {
        visit(node.children);
      }
    }
  };
  visit(root.children);
}

/**
 * Merges the physical text run of a rawtext/RCDATA element: markers
 * contribute nothing, text-only ranges aggregate, nested elements throw.
 * Shared by markup emission and layout records so both see one run.
 */
function collectVerbatimText(el: HtmlElement): string {
  let combined = "";
  const collect = (nodes: HtmlNode[]): void => {
    for (const child of nodes) {
      if (child.kind === "marker") {
        continue;
      }
      if (child.kind === "text") {
        combined += child.data;
        continue;
      }
      if (child.kind === "range") {
        collect(child.children);
        continue;
      }
      throw new HtmlRecordError(
        `Cannot serialize <${child.tag}> inside <${el.tag}>; the parser reads its content as text`,
        formatSite(child.meta.site),
        child.meta.site,
      );
    }
  };
  collect(el.children);
  if (hasClosingDelimiter(combined, el.tag)) {
    throw new HtmlRecordError(
      `Text content closes <${el.tag}> early`,
      "use a non-raw-text element or move the content into an external resource",
      el.meta.site,
    );
  }
  return combined;
}

/**
 * Rejects a live textarea value that would close the element early.
 * Shared by markup emission and layout records so both see the same value.
 */
function checkTextareaValue(el: HtmlElement, value: string): void {
  if (hasClosingDelimiter(value, "textarea")) {
    throw new HtmlRecordError(
      "Text content closes `<textarea>` early",
      "use a non-raw-text element or move the content into an external resource",
      el.meta.site,
    );
  }
}

function emitVerbatimText(el: HtmlElement, out: string[], escaped: boolean): void {
  const combined = collectVerbatimText(el);
  if (escaped && isLfStripTag(el.tag, el.ns) && combined.startsWith("\n")) {
    out.push("\n");
  }
  out.push(escaped ? escapeWith(combined, TextEscapes) : combined);
}

function emitAttributes(
  el: HtmlElement,
  out: string[],
  token: string | undefined,
  selectedOptions: Set<HtmlElement> | undefined,
): void {
  if (token !== undefined) {
    out.push(' data-rz="', escapeWith(token, AttributeEscapes), '"');
  }
  for (const attr of el.attributes.values()) {
    out.push(" ", attr.name);
    if (!attr.bool && attr.value !== undefined) {
      appendAttributeValue(out, attr.value);
    }
  }
  const classes = el.classState;
  if (classes.mode === "string") {
    out.push(" class");
    appendAttributeValue(out, classes.value);
  } else if (classes.mode === "tokens" && classes.tokens.size > 0) {
    out.push(' class="', escapeWith([...classes.tokens].join(" "), AttributeEscapes), '"');
  }
  const style = el.styleState;
  if (style.mode === "text") {
    out.push(" style");
    appendAttributeValue(out, style.cssText);
  } else if (style.mode === "map" && style.properties.size > 0) {
    const css = [...style.properties]
      .map(([property, value]) => `${property}: ${value};`)
      .join(" ");
    out.push(' style="', escapeWith(css, AttributeEscapes), '"');
  }
  if (el.ns !== "") {
    return;
  }
  const tag = el.tag.toLowerCase();
  if (el.hasValue && el.value !== undefined && (tag === "input" || tag === "option" || tag === "button")) {
    out.push(" value");
    appendAttributeValue(out, el.value);
  }
  if (el.checked && tag === "input") {
    out.push(" checked");
  }
  if (tag === "option") {
    if (selectedOptions !== undefined ? selectedOptions.has(el) : el.selected) {
      out.push(" selected");
    }
  }
}

/**
 * Resolves the hydration token an element emits, validating its charset.
 * Shared by markup emission and layout records so both accept the same roots.
 */
function resolveElementToken(
  el: HtmlElement,
  rootToken: string | undefined,
  isRoot: boolean,
): string | undefined {
  const token = isRoot && rootToken !== undefined ? rootToken : el.meta.token;
  if (token !== undefined && !isValidToken(token)) {
    throw new HtmlRecordError(
      `Invalid hydration token "${token}"; expected letters, digits, underscore or dot`,
      `<${el.tag}>`,
      el.meta.site,
    );
  }
  return token;
}

/**
 * Rejects record shapes the HTML parser cannot round-trip. Shared by markup
 * emission and layout records so neither path accepts a page the other drops.
 */
function checkElementShape(el: HtmlElement): void {
  if (el.ns === "" && el.tag.toLowerCase() === "colgroup" && el.attributes.has("span")) {
    const hasElements = el.children.some((child) => child.kind === "element");
    if (hasElements) {
      failLayout("carries `span` with element children, which the parser drops", "remove `span` or the children", el);
    }
  }
  if (isVoidTag(el.tag, el.ns)) {
    if (el.children.length > 0) {
      failLayout("is void but holds children, which the parser keeps as siblings", "move the children out", el);
    }
    if (el.innerHTML !== undefined && el.innerHTML !== "") {
      failLayout("is void but holds innerHTML, which cannot parse back", "move the content out", el);
    }
  }
  if (el.ns === "" && el.tag.toLowerCase() === "plaintext") {
    if (el.children.length > 0 || (el.innerHTML !== undefined && el.innerHTML !== "")) {
      throw new HtmlRecordError(
        "Cannot serialize content inside `<plaintext>`, which swallows the rest of the template as text",
        "render the text in a normal element instead",
        el.meta.site,
      );
    }
  }
}

function transportToken(token: string, tokens: ReadonlyMap<string, string> | undefined, site?: unknown): string {
  if (tokens === undefined) return token;
  const compact = tokens.get(token);
  if (compact === undefined || !isValidToken(compact)) {
    throw new HtmlRecordError(`Missing or invalid transport token for "${token}"`, formatSite(site), site);
  }
  return compact;
}

function emitElement(
  el: HtmlElement,
  out: string[],
  rootToken: string | undefined,
  isRoot: boolean,
  selectedOptions: Set<HtmlElement> | undefined,
  tokens: ReadonlyMap<string, string> | undefined,
): void {
  const token = resolveElementToken(el, rootToken, isRoot);
  checkElementShape(el);
  out.push("<", el.tag);
  const lower = el.ns === "" ? el.tag.toLowerCase() : "";
  const selectValue = el.ns === "" && lower === "select" && el.hasSelectValue ? el.selectValue : undefined;
  const forced = selectValue !== undefined ? matchSelectOption(el, selectValue) : undefined;
  emitAttributes(el, out, token === undefined ? undefined : transportToken(token, tokens, el.meta.site), selectedOptions);
  out.push(">");
  if (isVoidTag(el.tag, el.ns)) {
    return;
  }
  if (lower === "plaintext") {
    out.push("</", el.tag, ">");
    return;
  }
  if (el.innerHTML !== undefined) {
    out.push(el.innerHTML);
    out.push("</", el.tag, ">");
    return;
  }
  if (isRawtextTag(el.tag, el.ns)) {
    emitVerbatimText(el, out, false);
    out.push("</", el.tag, ">");
    return;
  }
  if (lower === "textarea" && el.hasValue && el.value !== undefined) {
    checkTextareaValue(el, el.value);
    if (el.value.startsWith("\n")) {
      out.push("\n");
    }
    out.push(escapeWith(el.value, TextEscapes));
    out.push("</", el.tag, ">");
    return;
  }
  if (isRcdataTag(el.tag, el.ns)) {
    emitVerbatimText(el, out, true);
    out.push("</", el.tag, ">");
    return;
  }
  const kind = containerKind(el);
  const kids = kind === "neutral" ? el.children : normalizeList(el.children, kind);
  if (isLfStripTag(el.tag, el.ns)) {
    const first = kids.find((kid) => lower !== "textarea" || kid.kind !== "marker");
    if (first !== undefined && first.kind === "text" && first.data.startsWith("\n")) {
      out.push("\n");
    }
  }
  const nested = lower === "select" ? forced : selectedOptions;
  for (const kid of kids) {
    emitChild(kid, out, undefined, false, nested, tokens);
  }
  out.push("</", el.tag, ">");
}

function matchSelectOption(select: HtmlElement, value: string): Set<HtmlElement> {
  const matched = new Set<HtmlElement>();
  const options: HtmlElement[] = [];
  collectOptions(select, options);
  for (const option of options) {
    const attribute = option.attributes.get("value");
    const current = option.hasValue && option.value !== undefined
      ? option.value
      : attribute === undefined ? optionText(option) : attribute.value ?? "";
    if (current === value) {
      matched.add(option);
      break;
    }
  }
  return matched;
}

function emitChild(
  node: EmitChild,
  out: string[],
  rootToken: string | undefined,
  isRoot: boolean,
  selectedOptions: Set<HtmlElement> | undefined,
  tokens: ReadonlyMap<string, string> | undefined,
): void {
  if (node.kind === "synthetic") {
    out.push("<", node.tag, ">");
    for (const kid of node.children) {
      emitChild(kid, out, undefined, false, selectedOptions, tokens);
    }
    out.push("</", node.tag, ">");
    return;
  }
  if (node.kind === "element") {
    const fresh = node.ns === "" && node.tag.toLowerCase() === "select";
    emitElement(node, out, rootToken, isRoot, fresh ? undefined : selectedOptions, tokens);
    return;
  }
  if (node.kind === "text") {
    out.push(escapeWith(node.data, TextEscapes));
    return;
  }
  if (node.kind === "marker") {
    out.push("<!>");
    return;
  }
  if (node.kind === "normalized") {
    emitRange(node.source, node.children, out, selectedOptions, tokens);
    return;
  }
  emitRange(node, node.children, out, selectedOptions, tokens);
}

function emitRange(
  range: HtmlRange,
  children: readonly EmitChild[],
  out: string[],
  selectedOptions: Set<HtmlElement> | undefined,
  tokens: ReadonlyMap<string, string> | undefined,
): void {
  if (!isValidToken(range.token)) {
    throw new HtmlRecordError(
      `Invalid hydration token "${range.token}"; expected letters, digits, underscore or dot`,
      formatSite(range.meta.site),
      range.meta.site,
    );
  }
  const token = transportToken(range.token, tokens, range.meta.site);
  out.push("<!--rz:1:", token, ":start-->");
  for (const kid of children) {
    emitChild(kid, out, undefined, false, selectedOptions, tokens);
  }
  out.push("<!--rz:1:", token, ":end-->");
}

/**
 * Serializes a settled record tree. `data-rz` is emitted for the root token
 * and for every nested element carrying its own `meta.token`, so nested
 * component template roots hydrate independently. Ranges outside
 * rawtext/RCDATA emit paired `rz:1` markers; table, select, colgroup and
 * RCDATA shapes are parser-normalized or rejected source-aware.
 */
export function serializeToString(root: HtmlElement, options?: HtmlSerializeOptions): string {
  const out: string[] = [];
  emitElement(root, out, options?.token, true, undefined, options?.ownerTokens);
  return out.join("");
}

/** Serializes sibling nodes without a root token for assembler fragments. */
export function serializeNodes(nodes: readonly HtmlNode[], ownerTokens?: ReadonlyMap<string, string>): string {
  const out: string[] = [];
  for (const node of nodes) {
    emitChild(node, out, undefined, false, undefined, ownerTokens);
  }
  return out.join("");
}

interface LayoutContext {
  readonly resolver: HtmlLayoutResolver | undefined;
  readonly rootToken: string | undefined;
}

function staticLayoutIndex(meta: { readonly staticIndex?: number }): number | undefined {
  return typeof meta.staticIndex === "number" ? meta.staticIndex : undefined;
}
function siteKeyOf(site: unknown): string | undefined {
  if (typeof site === "string") {
    return site;
  }
  if (typeof site === "object" && site !== null && "key" in site && typeof site.key === "string") {
    return site.key;
  }
  return undefined;
}

function layoutChild(node: EmitChild, ctx: LayoutContext): LayoutNode {
  switch (node.kind) {
    case "synthetic":
      return {
        kind: "element",
        tag: node.tag,
        ns: "",
        children: node.children.map((kid) => layoutChild(kid, ctx)),
      };
    case "element":
      return layoutElementNode(node, ctx, false);
    case "text": {
      const index = staticLayoutIndex(node.meta);
      return index === undefined
        ? { kind: "text", text: node.data }
        : { kind: "text", text: node.data, index };
    }
    case "marker": {
      const index = staticLayoutIndex(node.meta);
      return index === undefined ? { kind: "marker" } : { kind: "marker", index };
    }
    case "range":
      return layoutRangeNode(node, ctx, undefined);
    case "normalized":
      return layoutRangeNode(node.source, ctx, node.children);
  }
}

function layoutElementNode(el: HtmlElement, ctx: LayoutContext, isRoot: boolean): LayoutNode {
  const token = resolveElementToken(el, ctx.rootToken, isRoot);
  checkElementShape(el);
  const site = token === undefined ? undefined : ctx.resolver?.elementSite?.(el) ?? siteKeyOf(el.meta.site);
  const index = staticLayoutIndex(el.meta);
  let children: readonly LayoutNode[];
  let opaque: true | undefined;
  const lower = el.ns === "" ? el.tag.toLowerCase() : "";
  if (isVoidTag(el.tag, el.ns) || lower === "plaintext") {
    children = [];
  } else if (el.innerHTML !== undefined) {
    children = [];
    opaque = true;
  } else if (lower === "textarea" && el.hasValue && el.value !== undefined) {
    checkTextareaValue(el, el.value);
    children = [{ kind: "text", text: el.value }];
  } else if (isRawtextTag(el.tag, el.ns) || isRcdataTag(el.tag, el.ns)) {
    children = [{ kind: "text", text: collectVerbatimText(el) }];
  } else {
    const kind = containerKind(el);
    const kids: readonly EmitChild[] = kind === "neutral" ? el.children : normalizeList(el.children, kind);
    children = kids.map((kid) => layoutChild(kid, ctx));
  }
  return {
    kind: "element",
    tag: el.tag,
    ns: el.ns,
    ...(token === undefined ? {} : { token }),
    ...(site === undefined ? {} : { site }),
    ...(index === undefined ? {} : { index }),
    ...(opaque === undefined ? {} : { opaque }),
    children,
  };
}
function layoutRangeNode(range: HtmlRange, ctx: LayoutContext, kids: readonly EmitChild[] | undefined): LayoutNode {
  if (!isValidToken(range.token)) {
    throw new HtmlRecordError(
      `Invalid hydration token "${range.token}"; expected letters, digits, underscore or dot`,
      formatSite(range.meta.site),
      range.meta.site,
    );
  }
  const info = ctx.resolver?.rangeInfo?.(range);
  if (info === undefined) {
    throw new HtmlRecordError(
      `Cannot describe layout for range "${range.token}" without session owner binding`,
      "pass a resolver with rangeInfo",
      range.meta.site,
    );
  }
  const site = ctx.resolver?.rangeSite?.(range) ?? siteKeyOf(range.meta.site);
  const index = staticLayoutIndex(range.meta);
  return {
    kind: "range",
    token: range.token,
    ownerId: info.ownerId,
    ...(site === undefined ? {} : { site }),
    range: info.range,
    ...(info.placement === undefined ? {} : { placement: info.placement }),
    ...(index === undefined ? {} : { index }),
    children: (kids ?? range.children).map((kid) => layoutChild(kid, ctx)),
  };
}

/**
 * Describes the parser-normalized topology of a settled record tree as
 * hydration protocol layout nodes. Normalization is the same code markup
 * emission runs (`normalizeList`, `containerKind`, `collectVerbatimText`,
 * `checkElementShape`, `resolveElementToken`): implicit `tbody`/`tr`/`colgroup`
 * wrappers surface as plain element nodes, rawtext/RCDATA aggregate to one
 * text child, void/`plaintext`/opaque elements carry no children, and
 * foster-parented shapes throw instead of describing broken HTML. Layout-only
 * extras are attribute-free: element tokens come from the root option or
 * record meta, static indices and default site keys from record meta, and
 * range owner/kind/placement plus site overrides from the resolver. Ranges
 * inside rawtext/RCDATA stay transparent (their text merges into the single
 * run); every other range needs `rangeInfo`, otherwise description fails.
 */
export function describeLayout(root: HtmlElement, options?: HtmlLayoutOptions): LayoutNode[] {
  const ctx: LayoutContext = { resolver: options?.resolver, rootToken: options?.token };
  return [layoutElementNode(root, ctx, true)];
}

/** Describes sibling nodes without a root token for assembler fragments. */
export function describeNodes(nodes: readonly HtmlNode[], options?: HtmlLayoutOptions): LayoutNode[] {
  const ctx: LayoutContext = { resolver: options?.resolver, rootToken: undefined };
  return nodes.map((node) => layoutChild(node, ctx));
}

/**
 * Serializes markup and describes its parser-normalized layout from one
 * settled tree. Both outputs share the same normalization; callers needing
 * only one side keep using `serializeToString`/`serializeNodes` or
 * `describeLayout`/`describeNodes`.
 */
export function serializeWithLayout(
  root: HtmlElement,
  options?: HtmlLayoutOptions,
): { html: string; layout: LayoutNode[] } {
  return { html: serializeToString(root, options), layout: describeLayout(root, options) };
}
