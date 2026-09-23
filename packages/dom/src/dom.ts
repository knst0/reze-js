import { root, untrack } from "@rezejs/signals";
import { renderEffect as bind } from "@rezejs/signals/render";

import type { JSX } from "./jsx";

// Loosely typed on purpose: compiled output hangs `$$event` handlers and data off elements.
// oxlint-disable-next-line typescript/no-explicit-any
type Any = any;
type Props = Record<string, Any>;
/** What an insertion point currently holds: nothing, a text value, one node, or a node list. */
type Current = Any;

/**
 * `bind(fn, init)`: DOM binding that runs `fn` now and again when what it read changes, threading
 * its previous result. The compiler merges several updates of one element into one binding.
 * `memo`: cached `computed`, used by the compiler for conditions and child getters.
 */
export { renderEffect as bind } from "@rezejs/signals/render";
export { computed as memo } from "@rezejs/signals";

/** Events delegated to the document by default (SPECIFICATION §3.5). */
const DelegatedEvents: Record<string, 1> = {
  click: 1,
  input: 1,
  change: 1,
  submit: 1,
  keydown: 1,
  keyup: 1,
  pointerdown: 1,
  pointerup: 1,
  pointermove: 1,
  focusin: 1,
  focusout: 1,
};

/** DOM properties set as properties rather than attributes (SPECIFICATION §3.4). */
const Properties: Record<string, 1> = {
  value: 1,
  checked: 1,
  selected: 1,
  textContent: 1,
  innerHTML: 1,
};

/**
 * Parses `html` once (lazily, on first use) and returns a factory that deep-clones it.
 * SVG fragments arrive wrapped in `<svg>` and MathML ones need a MathML-namespaced parser.
 */
export function template(
  html: string,
  isImportNode?: boolean,
  isSVG?: boolean,
  isMathML?: boolean,
): () => Node {
  let node: Node | undefined;
  const create = (): Node => {
    const t = isMathML
      ? (document.createElementNS("http://www.w3.org/1998/Math/MathML", "template") as Any)
      : document.createElement("template");
    t.innerHTML = html;
    return isSVG ? t.content.firstChild.firstChild : isMathML ? t.firstChild : t.content.firstChild;
  };
  return isImportNode
    ? () => document.importNode((node ??= create()), true)
    : () => (node ??= create()).cloneNode(true);
}

/** Calls a component once, untracked: its reads never re-run the parent binding. */
export function createComponent<P>(Comp: (props: P) => JSX.Element, props: P): JSX.Element {
  return untrack(() => Comp(props));
}

/** Mounts `code()` into `element`; the returned function disposes it and clears the element. */
export function render(code: () => JSX.Element, element: Element): () => void {
  let dispose!: () => void;
  root((d) => {
    dispose = d;
    insert(element, code(), element.firstChild ? null : undefined);
  });
  return () => {
    dispose();
    element.textContent = "";
  };
}

// ---------------------------------------------------------------------------------------------
// Attributes and properties

/** `null`, `undefined` and `false` remove the attribute. */
export function setAttribute(node: Element, name: string, value: unknown): void {
  if (value == null || value === false) {
    node.removeAttribute(name);
  } else {
    node.setAttribute(name, value as string);
  }
}

export function setAttributeNS(node: Element, ns: string, name: string, value: unknown): void {
  if (value == null || value === false) {
    node.removeAttributeNS(ns, name.slice(name.indexOf(":") + 1));
  } else {
    node.setAttributeNS(ns, name, value as string);
  }
}

export function setBoolAttribute(node: Element, name: string, value: unknown): void {
  node.toggleAttribute(name, !!value);
}

export function setProperty(node: Element, name: string, value: unknown): void {
  (node as Any)[name] = value;
}

/** `class` value (Solid v2): a string, a toggle object, or an array mixing both; arrays nest. */
export type ClassValue =
  | string
  | number
  | boolean
  | null
  | undefined
  | Record<string, unknown>
  | ClassValue[];

/** `class={string | Record<string, boolean> | ClassValue[]}`; falsy items drop (except `0` → `"0"`). Object keys may hold several space-separated classes. Without `prev` (compiler-emitted updates) the previously applied tokens tracked on the element are diffed, so stale classes are removed. */
export function className(node: Element, value: unknown, prev?: unknown): void {
  if (value == null || value === false) {
    node.removeAttribute("class");
    (node as Any).$$class = undefined;
    return;
  }
  if (typeof value === "string") {
    if (value !== prev) node.setAttribute("class", value);
    (node as Any).$$class = undefined;
    return;
  }
  if (typeof prev === "string") {
    prev = undefined;
    node.removeAttribute("class");
    (node as Any).$$class = undefined;
  }
  applyClassTokens(
    node,
    "$$class",
    value as Record<string, unknown> & ClassValue[],
    prev as Record<string, unknown> | undefined,
  );
}

/** `classList` toggles each key; keys present in `prev` but gone from `value` are removed. Arrays flatten like `class`. */
export function classList(
  node: Element,
  value: Record<string, unknown> | ClassValue[] | null | undefined,
  prev?: Record<string, unknown> | ClassValue[],
): Record<string, unknown> | ClassValue[] | null | undefined {
  if (value != null) applyClassTokens(node, "$$classList", value, prev);
  else {
    const old: Record<string, unknown> = classListToObject(prev ?? (node as Any).$$classList ?? {});
    for (const key in old) if (key && key !== "undefined") node.classList.remove(key);
    (node as Any).$$classList = undefined;
  }
  return value;
}

/**
 * Diffs normalized `value` against the normalized `prev` (or the tokens this key applied last,
 * when the caller threads no `prev`), toggles what changed, and records the new tokens on the
 * element. Each key (`$$class`, `$$classList`) tracks only its own tokens, so the two never
 * clobber each other.
 */
function applyClassTokens(node: Element, key: string, value: unknown, prev?: unknown): void {
  const next = classListToObject(value);
  const old = classListToObject(prev ?? (node as Any)[key] ?? {});
  const list = node.classList;
  for (const k in old) {
    if (!k || k === "undefined" || next[k]) continue;
    list.remove(k);
  }
  for (const k in next) {
    if (!k || k === "undefined" || old[k] === !!next[k] || !next[k]) continue;
    list.add(k);
  }
  (node as Any)[key] = next;
}

/** Normalizes a `class`/`classList` value to one class token per `true` key. */
function classListToObject(value: unknown): Record<string, unknown> {
  if (Array.isArray(value)) {
    const result: Record<string, unknown> = {};
    flattenClassList(value, result);
    value = result;
  }
  if (value && typeof value === "object") {
    const result: Record<string, unknown> = {};
    for (const key of Object.keys(value)) {
      if (!(value as Record<string, unknown>)[key]) continue;
      for (const c of key.trim().split(/\s+/)) if (c) result[c] = true;
    }
    return result;
  }
  return {};
}

/** Flattens nested class arrays: objects merge, truthy strings/numbers become keys (`0` kept). */
function flattenClassList(list: unknown[], result: Record<string, unknown>): void {
  for (let i = 0; i < list.length; i++) {
    const item = list[i];
    if (Array.isArray(item)) flattenClassList(item, result);
    else if (typeof item === "object" && item !== null) Object.assign(result, item);
    else if (item || item === 0) result[item as string] = true;
  }
}

/** `style={string | Record<string, string | number>}` with kebab-case keys; returns the new `prev`. */
export function style(node: HTMLElement, value: unknown, prev?: unknown): unknown {
  const s = node.style;
  if (value == null || typeof value === "string") {
    if (value == null) node.removeAttribute("style");
    else s.cssText = value;
    return value;
  }
  const next = value as Record<string, unknown>;
  if (typeof prev === "object" && prev) {
    for (const key in prev) if (next[key] == null) s.removeProperty(key);
  } else {
    s.cssText = "";
    prev = {};
  }
  for (const key in next) {
    const v = next[key];
    if (v !== (prev as Props)[key]) setStyleProperty(node, key, v);
  }
  return next;
}

export function setStyleProperty(node: HTMLElement, name: string, value: unknown): void {
  if (value == null) node.style.removeProperty(name);
  else node.style.setProperty(name, value as string);
}

/** Runs a `ref` callback, untracked. */
export function use<T>(fn: (el: Element, arg?: T) => void, el: Element, arg?: T): void {
  untrack(() => fn(el, arg));
}

// ---------------------------------------------------------------------------------------------
// Events

const DelegatedKey = "$$events";

/** Installs one document-level listener per event type; handlers live on elements as `$$<type>`. */
export function delegateEvents(names: string[], doc: Document = window.document): void {
  const installed: Set<string> = ((doc as Any)[DelegatedKey] ??= new Set());
  for (const name of names) {
    if (!installed.has(name)) {
      installed.add(name);
      doc.addEventListener(name, eventHandler);
    }
  }
}

function eventHandler(e: Event): void {
  const key = "$$" + e.type;
  let node: Any = e.composedPath()[0] ?? e.target;
  // Each handler sees the element it was declared on, as with a direct listener.
  Object.defineProperty(e, "currentTarget", { configurable: true, get: () => node ?? document });
  // No `batch()`: writes schedule one microtask flush, so multiple handlers in this
  // dispatch — and separate dispatches in the same task — coalesce into one propagation.
  while (node) {
    const handler = node[key];
    if (handler && !node.disabled) {
      const data = node[key + "Data"];
      if (data !== undefined) handler.call(node, data, e);
      else handler.call(node, e);
      if (e.cancelBubble) return;
    }
    node = node.parentNode ?? node.host;
  }
}

/**
 * `delegate`: store the handler as `$$<name>` (an array is `[handler, data]`).
 * Otherwise a direct listener; an array is `[handler, options]` (`on:click={[fn, { passive: true }]}`).
 * Handler writes schedule one microtask flush and coalesce; use `flushSync` to read the DOM now.
 */
export function addEventListener(
  node: Element,
  name: string,
  handler: Any,
  delegate?: boolean,
): void {
  if (delegate) {
    if (Array.isArray(handler)) {
      (node as Any)["$$" + name] = handler[0];
      (node as Any)["$$" + name + "Data"] = handler[1];
    } else {
      (node as Any)["$$" + name] = handler;
    }
  } else {
    const [fn, options] = Array.isArray(handler) ? handler : [handler];
    if (fn) node.addEventListener(name, (e) => fn.call(node, e), options);
  }
}

// ---------------------------------------------------------------------------------------------
// Spread and props

/** Applies a props object to an element; reactive keys are re-applied when they change. */
export function spread(
  node: Element,
  props: Props = {},
  isSVG?: boolean,
  skipChildren?: boolean,
): void {
  const prev: Props = {};
  if (!skipChildren) insert(node, () => props.children);
  bind(() => typeof props.ref === "function" && use(props.ref, node));
  bind(() => {
    for (const key in props) {
      if (key === "children" || key === "ref") continue;
      const value = props[key];
      if (value !== prev[key]) prev[key] = assignProp(node, key, value, prev[key], isSVG);
    }
    for (const key in prev) {
      if (!(key in props)) prev[key] = assignProp(node, key, undefined, prev[key], isSVG);
    }
  });
}

function assignProp(node: Any, name: string, value: Any, prev: Any, isSVG?: boolean): Any {
  if (name === "style") return style(node, value, prev);
  if (name === "classList") return classList(node, value, prev);
  if (name === "class" || name === "className") {
    className(node, value, prev);
  } else if (name.startsWith("on")) {
    const custom = name[2] === ":";
    const type = custom ? name.slice(3) : name.slice(2).toLowerCase();
    if (!custom && Object.hasOwn(DelegatedEvents, type)) {
      node["$$" + type] = value;
      delegateEvents([type]);
    } else {
      const key = "$l" + type;
      if (node[key]) node.removeEventListener(type, node[key]);
      node[key] = value && ((e: Event) => value.call(node, e));
      if (value) node.addEventListener(type, node[key]);
    }
  } else if (name.startsWith("prop:")) {
    node[name.slice(5)] = value;
  } else if (name.startsWith("attr:")) {
    setAttribute(node, name.slice(5), value);
  } else if (name.startsWith("bool:")) {
    setBoolAttribute(node, name.slice(5), value);
  } else if (!isSVG && Object.hasOwn(Properties, name)) {
    node[name] = value;
  } else {
    setAttribute(node, name, value);
  }
  return value;
}

/**
 * Merges props objects; for each key the last source holding a non-`undefined` value wins.
 * Reads stay lazy, so getters keep their reactivity. Function sources (dynamic spreads) are
 * re-read on every access and may change their key set.
 */
export function mergeProps(...sources: Any[]): Props {
  const read = (key: PropertyKey): Any => {
    for (let i = sources.length; i--;) {
      let s = sources[i];
      if (typeof s === "function") s = s();
      const v = s?.[key];
      if (v !== undefined) return v;
    }
  };
  if (sources.some((s) => typeof s === "function")) {
    const resolved = () => sources.map((s) => (typeof s === "function" ? s() : s) ?? {});
    return new Proxy(
      {},
      {
        get: (_, key) => read(key),
        has: (_, key) => resolved().some((s) => key in s),
        ownKeys: () => [...new Set(resolved().flatMap((s) => Object.keys(s)))],
        getOwnPropertyDescriptor: (_, key) => ({
          configurable: true,
          enumerable: true,
          get: () => read(key),
        }),
      },
    );
  }
  const target: Props = {};
  for (const s of sources) {
    for (const key in s) {
      if (!(key in target)) {
        Object.defineProperty(target, key, {
          configurable: true,
          enumerable: true,
          get: () => read(key),
        });
      }
    }
  }
  return target;
}

/** Splits props by key groups into lazy views: one per group plus the rest. */
export function splitProps<T extends Props>(props: T, ...groups: (keyof T)[][]): Props[] {
  const out: Props[] = groups.map(() => ({}));
  const rest: Props = {};
  for (const key of Object.keys(props)) {
    const i = groups.findIndex((g) => g.includes(key));
    Object.defineProperty(i < 0 ? rest : out[i]!, key, {
      configurable: true,
      enumerable: true,
      get: () => props[key],
    });
  }
  out.push(rest);
  return out;
}

// ---------------------------------------------------------------------------------------------
// Children

/**
 * Inserts `value` into `parent` before `marker`. `marker === undefined` means `parent` holds
 * nothing else, so text and clearing can use `textContent`. A function becomes a binding.
 */
export function insert(parent: Node, value: Any, marker?: Node | null, initial?: Current): void {
  if (marker !== undefined && !initial) initial = [];
  if (typeof value !== "function") {
    insertExpression(parent, value, initial, marker);
  } else {
    bind((current) => insertExpression(parent, value(), current, marker), initial);
  }
}

/** Reconciles what `parent` shows at this insertion point with `value`; returns the new `current`. */
export function insertExpression(
  parent: Node,
  value: Any,
  current: Current,
  marker?: Node | null,
  unwrapArray?: boolean,
): Current {
  while (typeof current === "function") current = current();
  if (value === current) return current;
  const t = typeof value;
  const multi = marker !== undefined;
  parent = (multi && current[0]?.parentNode) || parent;

  if (t === "string" || t === "number" || t === "bigint") {
    value = String(value);
    if (multi) {
      let node = current[0];
      if (node?.nodeType === 3) {
        if (node.data !== value) node.data = value;
      } else {
        node = document.createTextNode(value);
      }
      current = cleanChildren(parent, current, marker, node);
    } else if (current !== "" && typeof current === "string") {
      current = (parent.firstChild as Text).data = value;
    } else {
      current = parent.textContent = value;
    }
  } else if (value == null || t === "boolean") {
    current = cleanChildren(parent, current, marker);
  } else if (t === "function") {
    bind(() => {
      let v = value();
      while (typeof v === "function") v = v();
      current = insertExpression(parent, v, current, marker);
    });
    return () => current;
  } else if (Array.isArray(value)) {
    const array: Node[] = [];
    if (normalizeIncomingArray(array, value, current, unwrapArray)) {
      bind(() => (current = insertExpression(parent, array, current, marker, true)));
      return () => current;
    }
    if (array.length === 0) {
      current = cleanChildren(parent, current, marker);
      if (multi) return current;
    } else if (Array.isArray(current)) {
      if (current.length === 0) appendNodes(parent, array, marker);
      else reconcileArrays(parent, current, array);
    } else {
      if (current) cleanChildren(parent);
      appendNodes(parent, array);
    }
    current = array;
  } else if (value.nodeType) {
    if (Array.isArray(current)) {
      if (multi) return cleanChildren(parent, current, marker, value);
      cleanChildren(parent, current, null, value);
    } else if (current == null || current === "" || !parent.firstChild) {
      parent.appendChild(value);
    } else {
      parent.replaceChild(value, parent.firstChild);
    }
    current = value;
  }
  return current;
}

/**
 * Flattens `array` into nodes, reusing text nodes from `current` at the same position.
 * Returns whether it met a function: then the caller must bind and resolve it (`unwrap`).
 */
function normalizeIncomingArray(
  normalized: Node[],
  array: Any[],
  current: Current,
  unwrap?: boolean,
): boolean {
  let dynamic = false;
  for (let i = 0; i < array.length; i++) {
    let item = array[i];
    const prev = current?.[normalized.length];
    const t = typeof item;
    if (item == null || t === "boolean") {
      // renders nothing
    } else if (t === "object" && item.nodeType) {
      normalized.push(item);
    } else if (Array.isArray(item)) {
      dynamic = normalizeIncomingArray(normalized, item, prev) || dynamic;
    } else if (t === "function") {
      if (unwrap) {
        while (typeof item === "function") item = item();
        dynamic =
          normalizeIncomingArray(
            normalized,
            Array.isArray(item) ? item : [item],
            Array.isArray(prev) ? prev : [prev],
          ) || dynamic;
      } else {
        normalized.push(item);
        dynamic = true;
      }
    } else {
      const value = String(item);
      if (prev?.nodeType === 3) {
        if (prev.data !== value) prev.data = value;
        normalized.push(prev);
      } else {
        normalized.push(document.createTextNode(value));
      }
    }
  }
  return dynamic;
}

function appendNodes(parent: Node, array: Node[], marker: Node | null = null): void {
  for (const node of array) parent.insertBefore(node, marker);
}

/**
 * Removes `current` (or everything, without a marker) and leaves `replacement`, or an empty text
 * node that keeps the insertion point's position, in its place.
 */
function cleanChildren(
  parent: Node,
  current?: Current,
  marker?: Node | null,
  replacement?: Node,
): Current {
  if (marker === undefined) return (parent.textContent = "");
  const node = replacement ?? document.createTextNode("");
  if (current.length) {
    let inserted = false;
    for (let i = current.length - 1; i >= 0; i--) {
      const el = current[i];
      if (node !== el) {
        const isParent = el.parentNode === parent;
        if (!inserted && !i) {
          if (isParent) parent.replaceChild(node, el);
          else parent.insertBefore(node, marker);
        } else if (isParent) {
          el.remove();
        }
      } else {
        inserted = true;
      }
    }
  } else {
    parent.insertBefore(node, marker);
  }
  return [node];
}

/**
 * Turns the sibling run `a` into `b` with few DOM moves: trims common ends, swaps crossed ends,
 * and moves runs that are already in order as a block.
 * Algorithm from udomdiff (ISC, Andrea Giammarchi) as adapted by dom-expressions (MIT, Ryan Carniato).
 */
export function reconcileArrays(parent: Node, a: Node[], b: Node[]): void {
  const bLength = b.length;
  let aEnd = a.length;
  let bEnd = bLength;
  let aStart = 0;
  let bStart = 0;
  const after = a[aEnd - 1]!.nextSibling;
  let map: Map<Node, number> | undefined;

  while (aStart < aEnd || bStart < bEnd) {
    if (a[aStart] === b[bStart]) {
      aStart++;
      bStart++;
      continue;
    }
    while (a[aEnd - 1] === b[bEnd - 1]) {
      aEnd--;
      bEnd--;
    }
    if (aEnd === aStart) {
      // only insertions left
      const node =
        bEnd < bLength ? (bStart ? b[bStart - 1]!.nextSibling : b[bEnd - bStart]!) : after;
      while (bStart < bEnd) parent.insertBefore(b[bStart++]!, node);
    } else if (bEnd === bStart) {
      // only removals left
      while (aStart < aEnd) {
        if (!map?.has(a[aStart]!)) (a[aStart] as ChildNode).remove();
        aStart++;
      }
    } else if (a[aStart] === b[bEnd - 1] && b[bStart] === a[aEnd - 1]) {
      // crossed ends: swap
      const node = a[--aEnd]!.nextSibling;
      parent.insertBefore(b[bStart++]!, a[aStart++]!.nextSibling);
      parent.insertBefore(b[--bEnd]!, node);
      a[aEnd] = b[bEnd]!;
    } else {
      if (!map) {
        map = new Map();
        for (let i = bStart; i < bEnd; i++) map.set(b[i]!, i);
      }
      const index = map.get(a[aStart]!);
      if (index == null) {
        (a[aStart++] as ChildNode).remove();
      } else if (bStart < index && index < bEnd) {
        let i = aStart;
        let sequence = 1;
        while (++i < aEnd && i < bEnd) {
          const t = map.get(a[i]!);
          if (t == null || t !== index + sequence) break;
          sequence++;
        }
        if (sequence > index - bStart) {
          const node = a[aStart]!;
          while (bStart < index) parent.insertBefore(b[bStart++]!, node);
        } else {
          parent.replaceChild(b[bStart++]!, a[aStart++]!);
        }
      } else {
        aStart++;
      }
    }
  }
}
