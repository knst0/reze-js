import { root, untrack } from "@rezejs/signals";
import { debugHook } from "@rezejs/signals/devtools";
import { renderEffect as bind } from "@rezejs/signals/render";

import { Hydration } from "./features";
import { nextHydrationKey, withComponentKeys, withKeyScope } from "./hydration";
import type { JSX } from "./jsx";
import { applyStreamChunks } from "./stream";

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
export const Properties: Record<string, 1> = {
  value: 1,
  checked: 1,
  selected: 1,
  textContent: 1,
  innerHTML: 1,
};

/**
 * Parses `html` once (lazily, on first use) and returns a factory that deep-clones it.
 * One flavor per namespace (B02): the compiler picks by the template root, so an
 * HTML-only app never ships the SVG/MathML parser branches.
 */
export function template(html: string): () => Node {
  let node: Node | undefined;
  const create = (): Node => {
    const t = document.createElement("template");
    t.innerHTML = html;
    return t.content.firstChild as Node;
  };
  return () => (node ??= create()).cloneNode(true) as Node;
}

/** `template` for `<svg>` roots: `html` arrives wrapped in `<svg>`. */
export function templateSVG(html: string): () => Node {
  let node: Node | undefined;
  const create = (): Node => {
    const t = document.createElement("template");
    t.innerHTML = html;
    const root = t.content.firstChild;
    return (root && root.firstChild) as Node;
  };
  return () => (node ??= create()).cloneNode(true) as Node;
}

/** `template` for `<math>` roots: parses under the MathML namespace. */
export function templateMathML(html: string): () => Node {
  let node: Node | undefined;
  const create = (): Node => {
    const t = document.createElementNS("http://www.w3.org/1998/Math/MathML", "template");
    t.innerHTML = html;
    return t.firstChild as Node;
  };
  return () => (node ??= create()).cloneNode(true) as Node;
}

/** Calls a component once, untracked: its reads never re-run the parent binding. */
export function createComponent<P>(Comp: (props: P) => JSX.Element, props: P): JSX.Element {
  if (process.env.NODE_ENV !== "production" && debugHook !== undefined) {
    return debugHook.component(Comp.name, () =>
      Hydration ? withComponentKeys(() => untrack(() => Comp(props))) : untrack(() => Comp(props)),
    );
  }
  if (!Hydration) return untrack(() => Comp(props));
  return withComponentKeys(() => untrack(() => Comp(props)));
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
// Hydration: code compiled for the `hydrate` target claims the DOM `renderToString` produced.

/** Server-rendered template roots by `data-hk`, while `hydrate` runs. */
let claimable: Map<string, Element> | undefined;

/**
 * Like `render`, but adopts the server-rendered DOM in `element` instead of replacing it: each
 * template claims its element by hydration key and each insert takes over what the server
 * rendered in its place. What cannot be claimed is created and reconciled as usual.
 */
export function hydrate(code: () => JSX.Element, element: Element): () => void {
  applyStreamChunks(element);
  claimable = new Map();
  for (const node of element.querySelectorAll("[data-hk]")) addClaimable(node);
  let dispose!: () => void;
  try {
    withKeyScope("", () =>
      root((d) => {
        dispose = d;
        insert(element, code(), undefined, renderedContent(element));
      }),
    );
  } finally {
    claimable = undefined;
  }
  return () => {
    dispose();
    element.textContent = "";
  };
}

function addClaimable(node: Element): void {
  claimable!.set(node.getAttribute("data-hk")!, node);
}
/** Hydrates an island itself once its trigger fires; see `lazyIsland`. */
export interface LazyIsland {
  hydrate(
    island: ServerIsland,
    props: Props,
    state: IslandsState,
    onPending: (cancel: () => void) => void,
  ): void;
}

/** A component or a `lazyIsland(...)` in the `hydrateIslands` map. */
export type IslandValue = ((props: Any) => JSX.Element) | LazyIsland;

interface SlotRange {
  name: string;
  nodes: Node[];
}

export interface ServerIsland {
  open: Comment;
  close: Comment;
  value: IslandValue;
  scope: string;
  props: Props;
  slots: SlotRange[];
}

/**
 * Hydrates only the islands `renderToString(code, true)` marked inside `element`: each runs its
 * component in the key scope the server rendered it in and adopts the nodes between its
 * `<!--$id:scope:props:mode-->` and `<!--/$-->` markers. A slot range
 * `<!--$slot:name-->…<!--/$slot-->` becomes an array of the nodes of its first range
 * (`undefined` when empty); every insert of the slot moves those nodes, like one value
 * inserted twice. Eager islands hydrate at once, in document order;
 * lazy ones (`lazyIsland(load, mode, export)`) load their module first: `idle` waits for
 * `requestIdleCallback` (`setTimeout` fallback), `visible` for an `IntersectionObserver` on the
 * first element after the opening marker (else the parent), `interaction` for a capture
 * `pointerdown`/`focusin`/`keydown` on the parent — and any of those events inside the island
 * loads it at once in every lazy mode. Delegated events inside a lazy island before it hydrates
 * are recorded (the last 32; a submit, and a click that would navigate or submit, have their
 * default prevented) and dispatched again on the same elements once it hydrated, unless
 * `options.replay` is `false`.
 * Throws when `islands` lacks an id found in the markup; the returned function disposes every
 * hydrated island, cancels the pending ones, and leaves the DOM as is.
 */
export function hydrateIslands(
  element: Element,
  islands: Record<string, IslandValue>,
  options?: { replay?: boolean },
): () => void {
  applyStreamChunks(element);
  const found = collectIslands(element, islands);
  return root((dispose) => {
    const state: IslandsState = { cancelled: false, replay: options?.replay !== false };
    const cancellations: (() => void)[] = [];
    for (const island of found) hydrateFound(island, state, cancellations.push.bind(cancellations));
    return () => {
      state.cancelled = true;
      for (const cancel of cancellations) cancel();
      dispose();
    };
  });
}

/** Every island marker pair in document order, with its slot ranges; nested islands inside slots included. */
function collectIslands(element: Element, islands: Record<string, IslandValue>): ServerIsland[] {
  interface Frame {
    open: Comment;
    value: IslandValue;
    scope: string;
    props: Props;
    slots: SlotRange[];
    openSlot: { name: string; open: Comment } | undefined;
  }
  const found: ServerIsland[] = [];
  const stack: Frame[] = [];
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_COMMENT);
  while (walker.nextNode()) {
    const node = walker.currentNode as Comment;
    const marker = node.data;
    if (marker === IslandClose) {
      const frame = stack.pop();
      if (frame) found.push({ ...frame, close: node });
    } else if (marker === SlotClose) {
      const frame = stack[stack.length - 1];
      const openSlot = frame?.openSlot;
      if (!frame || !openSlot) continue;
      frame.openSlot = undefined;
      const nodes: Node[] = [];
      for (let n = openSlot.open.nextSibling!; n !== node; n = n.nextSibling!) nodes.push(n);
      frame.slots.push({ name: openSlot.name, nodes });
    } else if (marker.startsWith(SlotOpen)) {
      const frame = stack[stack.length - 1];
      if (!frame || frame.openSlot) continue;
      frame.openSlot = { name: decodeURIComponent(marker.slice(SlotOpen.length)), open: node };
    } else if (marker[0] === IslandOpen) {
      const parsed = parseIslandMarker(marker);
      if (!parsed) continue;
      if (!Object.hasOwn(islands, parsed.id)) {
        throw new Error(`hydrateIslands: no component for island "${parsed.id}"`);
      }
      stack.push({
        open: node,
        value: islands[parsed.id]!,
        scope: parsed.scope,
        props: parsed.props,
        slots: [],
        openSlot: undefined,
      });
    }
  }
  return found;
}

/** `<!--$id:scope:json:mode-->`; `mode` is the text after the last colon. */
function parseIslandMarker(
  marker: string,
): { id: string; scope: string; props: Props; mode: string } | undefined {
  const idEnd = marker.indexOf(":");
  const modeEnd = marker.lastIndexOf(":");
  if (marker[0] !== IslandOpen || idEnd < 0 || modeEnd <= idEnd) return undefined;
  const middle = marker.slice(idEnd + 1, modeEnd);
  const scopeEnd = middle.indexOf(":");
  if (scopeEnd < 0) return undefined;
  return {
    id: marker.slice(1, idEnd),
    scope: middle.slice(0, scopeEnd),
    props: JSON.parse(middle.slice(scopeEnd + 1)),
    mode: marker.slice(modeEnd + 1),
  };
}

export interface IslandsState {
  cancelled: boolean;
  replay: boolean;
}

function hydrateFound(
  island: ServerIsland,
  state: IslandsState,
  onPending: (cancel: () => void) => void,
): void {
  const props = { ...island.props };
  for (const slot of island.slots) {
    if (!(slot.name in props)) props[slot.name] = slot.nodes.length ? slot.nodes : undefined;
  }
  if (typeof island.value === "function") {
    hydrateNow(island, island.value, props);
    return;
  }
  island.value.hydrate(island, props, state, onPending);
}

export function hydrateNow(
  island: ServerIsland,
  render: (props: Any) => JSX.Element,
  props: Props,
): void {
  const { open, close, scope } = island;
  const current: Node[] = [];
  claimable = new Map();
  for (let node = open.nextSibling!; node !== close; node = node.nextSibling!) {
    current.push(node);
    if (node.nodeType !== 1) continue;
    if ((node as Element).hasAttribute("data-hk")) addClaimable(node as Element);
    for (const claimed of (node as Element).querySelectorAll("[data-hk]")) addClaimable(claimed);
  }
  try {
    withKeyScope(scope, () =>
      insert(
        open.parentNode!,
        untrack(() => render(props)),
        close,
        current,
      ),
    );
  } finally {
    claimable = undefined;
  }
}

/** The server-rendered root of this template, or a fresh clone when there is none to claim. */
export function claim(template: () => Node, tag: string): Node {
  const key = claimable && nextHydrationKey();
  const node = key === undefined ? undefined : claimable!.get(key);
  if (node?.localName === tag) {
    claimable!.delete(key!);
    return node;
  }
  return template();
}

const InsertOpen = "[";
const InsertClose = "]";
export const IslandOpen = "$";
export const IslandClose = "/$";
/** `<!--$slot:<percent-encoded name>-->` opens the range of one island slot. */
export const SlotOpen = "$slot:";
export const SlotClose = "/$slot";

function isInsertMarker(node: Node | null, data: string): boolean {
  return node?.nodeType === 8 && (node as Comment).data === data;
}

/** `node`, or the first node after the server-rendered inserts starting at `node`. */
function skipInserts(node: Node | null): Node | null {
  while (isInsertMarker(node, InsertOpen)) {
    let depth = 1;
    while (depth) {
      node = node!.nextSibling;
      if (isInsertMarker(node, InsertOpen)) depth++;
      else if (isInsertMarker(node, InsertClose)) depth--;
    }
    node = node!.nextSibling;
  }
  return node;
}

/** The `index`-th template child of `parent`, stepping over server-rendered inserts. */
export function claimChild(parent: Node, index: number): Node {
  let node = skipInserts(parent.firstChild);
  while (index--) node = skipInserts(node!.nextSibling);
  return node!;
}

/** The template node `count` siblings after `node`, stepping over server-rendered inserts. */
export function claimSibling(node: Node, count: number): Node {
  while (count--) node = skipInserts(node.nextSibling)!;
  return node;
}

/** The `<!--[-->` opening the insert that `close` ends. */
function insertOpening(close: Node): Node | null {
  let depth = 0;
  for (let node = close.previousSibling; node; node = node.previousSibling) {
    if (isInsertMarker(node, InsertClose)) depth++;
    else if (isInsertMarker(node, InsertOpen) && !depth--) return node;
  }
  return null;
}

/**
 * `insert` that takes over what the server rendered for it: the whole content of `parent` for
 * a sole child (`marker === undefined`), otherwise the `<!--[-->…<!--]-->` range before
 * `marker` (or at the end), skipping the ranges of the `insertsAfter` later inserts that share
 * the marker. Without such a range it is a plain `insert`.
 */
export function claimInsert(
  parent: Node,
  value: Any,
  marker?: Node | null,
  insertsAfter = 0,
): void {
  if (marker === undefined) {
    insert(parent, value, undefined, renderedContent(parent));
    return;
  }
  let close = marker ? marker.previousSibling : parent.lastChild;
  for (let i = 0; i < insertsAfter && isInsertMarker(close, InsertClose); i++) {
    close = insertOpening(close!)?.previousSibling ?? null;
  }
  const open = isInsertMarker(close, InsertClose) ? insertOpening(close!) : null;
  if (!open) {
    insert(parent, value, marker);
    return;
  }
  const current: Node[] = [];
  for (let node = open.nextSibling!; node !== close; node = node.nextSibling!) current.push(node);
  insert(parent, value, close, current);
}

/** What `parent` shows, as the `current` of an insert that owns all of it. */
function renderedContent(parent: Node): Current {
  const first = parent.firstChild;
  if (!first) return undefined;
  if (first.nextSibling) return [...parent.childNodes];
  return first.nodeType === 3 ? (first as Text).data : first;
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
  applyClassTokens(node, value, prev);
}

/**
 * Diffs normalized `value` against the normalized `prev` (or the tokens applied last, tracked
 * on the element as `$$class`, when the caller threads no `prev`), toggles what changed, and
 * records the new tokens.
 */
function applyClassTokens(node: Element, value: unknown, prev?: unknown): void {
  const next = classTokens(value);
  const old = classTokens(prev ?? (node as Any).$$class ?? {});
  const list = node.classList;
  for (const k in old) {
    if (!k || k === "undefined" || next[k]) continue;
    list.remove(k);
  }
  for (const k in next) {
    if (!k || k === "undefined" || old[k] === !!next[k] || !next[k]) continue;
    list.add(k);
  }
  (node as Any).$$class = next;
}

/** Normalizes a `class` value to one class token per `true` key. */
export function classTokens(value: unknown): Record<string, unknown> {
  if (Array.isArray(value)) {
    const result: Record<string, unknown> = {};
    flattenClassValue(value, result);
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
function flattenClassValue(list: unknown[], result: Record<string, unknown>): void {
  for (let i = 0; i < list.length; i++) {
    const item = list[i];
    if (Array.isArray(item)) flattenClassValue(item, result);
    else if (typeof item === "object" && item !== null) Object.assign(result, item);
    else if (item || item === 0) result[item as string] = true;
  }
}

/** Adds or removes one class token when `isOn` differs from `wasOn` (never set counts as off); returns `isOn`. */
export function toggleClass(node: Element, token: string, isOn: boolean, wasOn?: boolean): boolean {
  if (isOn !== !!wasOn) node.classList.toggle(token, isOn);
  return isOn;
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

/** Runs a `ref` callback, untracked. The element type infers from the callback (D10). */
export function use<E extends Element, T>(fn: (el: E, arg?: T) => void, el: E, arg?: T): void {
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
  if (!skipChildren) insert(node, () => props.children, undefined, renderedContent(node));
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
  if (name === "class") {
    className(node, value, prev);
  } else if (name.startsWith("on")) {
    const custom = name[2] === ":";
    const lowered = custom ? name.slice(3) : name.slice(2).toLowerCase();
    const type = !custom && lowered === "doubleclick" ? "dblclick" : lowered;
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

/**
 * A live view of `props` without `keys` (or without the keys `hidden` accepts): nothing is read
 * until a key is used, and getters stay reactive. Spreading the view skips the hidden keys.
 */
export function omit<T extends Props, K extends readonly (keyof T)[]>(
  props: T,
  ...keys: K
): Omit<T, K[number]>;
export function omit<T extends Props>(
  props: T,
  hidden: (key: keyof T & (string | symbol)) => boolean,
): Partial<T>;
export function omit(props: Props, ...keys: unknown[]): Props {
  const first = keys[0];
  const isHidden: (key: PropertyKey) => boolean =
    typeof first === "function"
      ? (first as (key: PropertyKey) => boolean)
      : (key) => keys.includes(key);
  return new Proxy(props, {
    get: (target, key, receiver) =>
      isHidden(key) ? undefined : Reflect.get(target, key, receiver),
    has: (target, key) => !isHidden(key) && Reflect.has(target, key),
    ownKeys: (target) => Reflect.ownKeys(target).filter((key) => !isHidden(key)),
    getOwnPropertyDescriptor: (target, key) =>
      isHidden(key) ? undefined : Reflect.getOwnPropertyDescriptor(target, key),
  });
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
