import { Properties } from "../../../../crates/reze_compiler/src/html-data.json";

import {
  type HtmlElement,
  HtmlRecordError,
  type HtmlText,
  attachChild,
  clearChildren,
  createText,
  formatSite,
} from "./tree";

/** Class input mirroring the DOM `className` helper's accepted values. */
export type HtmlClassValue =
  | string
  | number
  | boolean
  | null
  | undefined
  | Record<string, unknown>
  | ReadonlyArray<HtmlClassValue>;

/** Style input mirroring the DOM `style` helper's accepted values. */
export type HtmlStyleValue =
  | string
  | { readonly [property: string]: string | number | null | undefined }
  | null
  | undefined;

export interface HtmlSpreadOptions {
  isSvg?: boolean;
  hasChildren?: boolean;
}

const Whitespace = /\s/;
const WhitespaceRun = /\s+/;
const AttributeNamePattern = /^[^\s"'`>/=<]+$/;

function lowerTag(el: HtmlElement): string {
  return el.tag.toLowerCase();
}

function isHtml(el: HtmlElement): boolean {
  return el.ns === "";
}

function checkAttributeName(name: string, site?: unknown): void {
  if (!AttributeNamePattern.test(name)) {
    throw new HtmlRecordError(
      `Invalid attribute name "${name}"`,
      formatSite(site),
      site,
    );
  }
}

function coerceAttribute(value: unknown, name: string, site?: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "bigint" || value === true) {
    return String(value);
  }
  throw new HtmlRecordError(
    `Cannot render ${typeof value} as attribute "${name}"; pass a string, number, bigint or boolean, or null/undefined/false to remove it`,
    formatSite(site),
    site,
  );
}

function coerceValueText(value: unknown, name: string, site?: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (
    typeof value === "number" ||
    typeof value === "bigint" ||
    typeof value === "boolean" ||
    value === null ||
    value === undefined
  ) {
    return String(value);
  }
  throw new HtmlRecordError(
    `Cannot render ${typeof value} as property "${name}"; pass a scalar value`,
    formatSite(site),
    site,
  );
}

/**
 * Sets a plain attribute; `null`, `undefined` and `false` remove it.
 * `class` and `style` funnel into their dedicated states so last-write wins.
 */
export function setAttribute(el: HtmlElement, name: string, value: unknown, site?: unknown): void {
  checkAttributeName(name, site);
  if (value === null || value === undefined || value === false) {
    el.attributes.delete(name);
    return;
  }
  if (name === "class") {
    setClass(el, value as HtmlClassValue, site);
    return;
  }
  if (name === "style") {
    setStyle(el, value, undefined, site);
    return;
  }
  el.attributes.set(name, {
    name,
    ns: undefined,
    bool: false,
    value: coerceAttribute(value, name, site),
  });
}

export function removeAttribute(el: HtmlElement, name: string): void {
  el.attributes.delete(name);
}

/**
 * Sets a namespaced attribute; `null`, `undefined` and `false` remove it.
 * `name` stays qualified (`xlink:href`).
 */
export function setAttributeNS(
  el: HtmlElement,
  ns: string,
  name: string,
  value: unknown,
  site?: unknown,
): void {
  checkAttributeName(name, site);
  if (value === null || value === undefined || value === false) {
    el.attributes.delete(name);
    return;
  }
  el.attributes.set(name, {
    name,
    ns,
    bool: false,
    value: coerceAttribute(value, name, site),
  });
}

/** Toggles a boolean attribute by truthiness. */
export function setBoolAttribute(el: HtmlElement, name: string, value: unknown, site?: unknown): void {
  checkAttributeName(name, site ?? el.meta.site);
  if (value) {
    el.attributes.set(name, { name, ns: undefined, bool: true, value: undefined });
  } else {
    el.attributes.delete(name);
  }
}

function valueHosts(el: HtmlElement): boolean {
  if (!isHtml(el)) {
    return false;
  }
  const tag = lowerTag(el);
  return tag === "input" || tag === "option" || tag === "button";
}

/**
 * Sets an IDL-style property. The representable form state feeds the
 * serializer: `value` on `input`/`option`/`button` becomes the value
 * attribute, `textarea` value becomes its escaped text content, late `select`
 * value resolves onto the matching option, `checked` on `input` and
 * `selected` on `option` become bare attributes, `textContent` replaces
 * children and `innerHTML` goes opaque. Every other `prop:` becomes
 * client-only state reapplied by the parent layer after hydration and never
 * reaches the HTML bytes.
 */
export function setProperty(el: HtmlElement, name: string, value: unknown, site?: unknown): void {
  if (name === "innerHTML") {
    setInnerHTML(el, value, site);
    return;
  }
  if (name === "textContent") {
    setTextContent(el, value, site);
    return;
  }
  if (name === "value") {
    if (isHtml(el) && lowerTag(el) === "select") {
      el.selectValue = coerceValueText(value, name, site);
      el.hasSelectValue = true;
      return;
    }
    if (valueHosts(el) || (isHtml(el) && lowerTag(el) === "textarea")) {
      el.value = coerceValueText(value, name, site);
      el.hasValue = true;
      return;
    }
    el.genericProps.set(name, value);
    return;
  }
  if (name === "checked") {
    if (isHtml(el) && lowerTag(el) === "input") {
      el.checked = !!value;
      return;
    }
    el.genericProps.set(name, value);
    return;
  }
  if (name === "selected") {
    if (isHtml(el) && lowerTag(el) === "option") {
      el.selected = !!value;
      return;
    }
    el.genericProps.set(name, value);
    return;
  }
  el.genericProps.set(name, value);
}

/**
 * Replaces children with a single text record; `null` and `undefined` clear.
 * Mirrors the DOM `textContent` setter coercion without object stringification.
 */
export function setTextContent(el: HtmlElement, value: unknown, site?: unknown): void {
  el.innerHTML = undefined;
  if (value === null || value === undefined) {
    clearChildren(el);
    return;
  }
  const text = createText(coerceValueText(value, "textContent", site));
  clearChildren(el);
  attachChild(el, text, site);
}

/**
 * Installs an opaque HTML subtree; structural attach afterwards fails.
 * Mirrors the DOM `innerHTML` setter coercion (`null` clears to empty,
 * `undefined` becomes `"undefined"`) without object stringification.
 */
export function setInnerHTML(el: HtmlElement, html: unknown, site?: unknown): void {
  clearChildren(el);
  if (html === null) {
    el.innerHTML = "";
    return;
  }
  el.innerHTML = coerceValueText(html, "innerHTML", site);
}

function addToggleTokens(classes: string, on: unknown, tokens: Set<string>): void {
  if (!on) {
    return;
  }
  if (!Whitespace.test(classes)) {
    if (classes !== "" && classes !== "__proto__") {
      tokens.add(classes);
    }
    return;
  }
  for (const token of classes.split(WhitespaceRun)) {
    if (token !== "" && token !== "__proto__") {
      tokens.add(token);
    }
  }
}

function mergeClassList(list: ReadonlyArray<HtmlClassValue>, merged: Map<string, unknown>): void {
  for (const item of list) {
    if (Array.isArray(item)) {
      mergeClassList(item, merged);
    } else if (typeof item === "object" && item !== null) {
      for (const key of Object.keys(item)) {
        merged.set(key, (item as Record<string, unknown>)[key]);
      }
    } else if (item || item === 0) {
      merged.set(String(item), true);
    }
  }
}

/**
 * Applies a `class` value with the DOM helper's semantics: a string replaces,
 * `null`/`undefined`/`false` clear, toggle objects and arrays merge in order
 * where a later key overrides an earlier one.
 */
export function setClass(el: HtmlElement, value: HtmlClassValue, site?: unknown): void {
  if (value === null || value === undefined || value === false) {
    el.classState = { mode: "none" };
    return;
  }
  if (typeof value === "string") {
    el.classState = { mode: "string", value };
    return;
  }
  const tokens = new Set<string>();
  if (Array.isArray(value)) {
    const merged = new Map<string, unknown>();
    mergeClassList(value, merged);
    for (const [classes, on] of merged) {
      addToggleTokens(classes, on, tokens);
    }
  } else if (typeof value === "object" && value !== null) {
    for (const classes in value) {
      addToggleTokens(classes, (value as Record<string, unknown>)[classes], tokens);
    }
  }
  void site;
  el.classState = { mode: "tokens", tokens };
}

function classTokenSet(el: HtmlElement): Set<string> {
  const state = el.classState;
  if (state.mode === "tokens") {
    return state.tokens;
  }
  const tokens = new Set<string>();
  if (state.mode === "string") {
    addToggleTokens(state.value, true, tokens);
  }
  el.classState = { mode: "tokens", tokens };
  return tokens;
}

/**
 * Flips one class token, preserving tokens set by other writers. Mirrors the
 * DOM helper including the `wasOn` diff guard; returns the next state.
 */
export function toggleClass(el: HtmlElement, token: string, isOn: unknown, wasOn?: unknown): boolean {
  if (token === "" || Whitespace.test(token)) {
    throw new HtmlRecordError(
      `Invalid class token "${token}"; expected a single non-empty token`,
      formatSite(el.meta.site),
      el.meta.site,
    );
  }
  const on = !!isOn;
  if (on !== !!wasOn) {
    const tokens = classTokenSet(el);
    if (on) {
      tokens.add(token);
    } else {
      tokens.delete(token);
    }
  }
  return on;
}

function coerceStyleValue(value: string | number, property: string, site?: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number") {
    return String(value);
  }
  throw new HtmlRecordError(
    `Cannot render ${typeof value} as style property "${property}"; pass a string or number, or null/undefined to remove it`,
    formatSite(site),
    site,
  );
}

/**
 * Applies a `style` value with the DOM helper's semantics: `null` and
 * `undefined` remove the style, a string becomes cssText, an object diffs
 * against `prev` so only changed properties are written. Returns `value` for
 * prev-chaining.
 */
export function setStyle(el: HtmlElement, value: unknown, prev?: unknown, site?: unknown): unknown {
  if (value === null || value === undefined) {
    el.styleState = { mode: "none" };
    return value;
  }
  if (typeof value === "string") {
    el.styleState = { mode: "text", cssText: value };
    return value;
  }
  if (typeof value !== "object") {
    el.styleState = { mode: "text", cssText: String(value) };
    return value;
  }
  const input = value as Record<string, string | number | null | undefined>;
  const last =
    typeof prev === "object" && prev !== null
      ? (prev as Record<string, string | number | null | undefined>)
      : undefined;
  const properties =
    last !== undefined && el.styleState.mode === "map"
      ? new Map(el.styleState.properties)
      : new Map<string, string>();
  if (last !== undefined) {
    for (const property in last) {
      if (input[property] === null || input[property] === undefined) {
        properties.delete(property);
      }
    }
  }
  for (const property in input) {
    const next = input[property];
    if (next !== null && next !== undefined && (last === undefined || next !== last[property])) {
      if (property === "") {
        throw new HtmlRecordError("Invalid empty style property name", formatSite(site), site);
      }
      properties.set(property, coerceStyleValue(next, property, site));
    }
  }
  el.styleState = { mode: "map", properties };
  return value;
}

/**
 * Writes a text record with the native `Text.data` conversion: scalars render
 * (`false` becomes `"false"`, `null` clears to `""`), unrepresentable object,
 * function and symbol values fail instead of stringifying.
 */
export function setTextDataValue(node: HtmlText, value: unknown, site?: unknown): void {
  node.data = coerceTextData(value, site);
}

function coerceTextData(value: unknown, site?: unknown): string {
  if (value === null) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }
  if (
    typeof value === "number" ||
    typeof value === "bigint" ||
    typeof value === "boolean" ||
    value === undefined
  ) {
    return String(value);
  }
  throw new HtmlRecordError(
    `Cannot render ${typeof value} as text data; pass a string, number, bigint or boolean`,
    formatSite(site),
    site,
  );
}

/**
 * Normalizes an inserted child value with the `insert` expression rules: only
 * string/number/bigint render, `null`/`undefined`/boolean render nothing and
 * are reported as `undefined` for the parent layer to skip node creation.
 */
export function normalizeInsertedText(value: unknown, site?: unknown): string | undefined {
  if (value === null || value === undefined || typeof value === "boolean") {
    return undefined;
  }
  if (typeof value === "string" || typeof value === "number" || typeof value === "bigint") {
    return String(value);
  }
  throw new HtmlRecordError(
    `Cannot render ${typeof value} as inserted text; pass a string, number or bigint`,
    formatSite(site),
    site,
  );
}

function assignSpreadProp(
  el: HtmlElement,
  name: string,
  value: unknown,
  prev: unknown,
  isSvg: boolean,
  site?: unknown,
): void {
  if (name === "style") {
    setStyle(el, value, prev, site);
    return;
  }
  if (name === "class") {
    setClass(el, value as HtmlClassValue, site);
    return;
  }
  if (name.startsWith("on")) {
    return;
  }
  if (name.startsWith("prop:")) {
    setProperty(el, name.slice(5), value, site);
    return;
  }
  if (name.startsWith("attr:")) {
    setAttribute(el, name.slice(5), value, site);
    return;
  }
  if (name.startsWith("bool:")) {
    setBoolAttribute(el, name.slice(5), value, site);
    return;
  }
  if (!isSvg && Object.hasOwn(Properties, name)) {
    setProperty(el, name, value, site);
    return;
  }
  setAttribute(el, name, value, site);
}

/**
 * Applies one spread segment with `spread.ts` routing and last-write-wins
 * order. Event listeners and `ref` are client work owned by the parent layer
 * and skipped here. `children` is returned for the parent helper to insert
 * unless `hasChildren` is set; keys applied by a previous call but absent now
 * are removed, mirroring the DOM spread effect. Removal tracking defaults to
 * the shared element map, but distinct spread bindings on one element must
 * pass their own map (like each CSR `spread` call owns its `applied`
 * closure) so one binding cannot remove another binding's keys.
 */
export function applySpread(
  el: HtmlElement,
  props: unknown,
  options?: HtmlSpreadOptions,
  site?: unknown,
  previous?: Map<string, unknown>,
): { children: unknown; insertChildren: boolean } {
  const isSvg = options?.isSvg ?? el.ns !== "";
  const insertChildren = !(options?.hasChildren ?? false);
  const source = (props ?? {}) as Record<string, unknown>;
  const prev = previous ?? el.spreadPrev;
  const seen = new Set<string>();
  for (const name in source) {
    if (name === "children" || name === "ref") {
      continue;
    }
    seen.add(name);
    const value = source[name];
    if (value !== prev.get(name)) {
      assignSpreadProp(el, name, value, prev.get(name), isSvg, site);
      prev.set(name, value);
    }
  }
  for (const name of [...prev.keys()]) {
    if (prev.get(name) !== undefined && !seen.has(name) && !(name in source)) {
      assignSpreadProp(el, name, undefined, prev.get(name), isSvg, site);
      prev.delete(name);
    }
  }
  return { children: source["children"], insertChildren };
}
