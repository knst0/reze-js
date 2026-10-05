import { computed, root, untrack } from "@rezejs/signals";
import type { ContinuationHandle } from "@rezejs/signals/internal/continuation";
import { internalAsyncComputed, type AsyncContext } from "@rezejs/signals/internal/resource";
import { renderEffect } from "@rezejs/signals/render";

import { SVGElements } from "../../../../crates/reze_compiler/src/html-data.json";
import { errored } from "../errored";
import { branch, choose } from "../flow";
import { currentExecution, type Instance } from "../hydration/execution";
import { HydrationError, type RangeKind, type Site } from "../hydration/protocol";
import type { JSX } from "../jsx";
import { list } from "../list";
import { loading } from "../loading";
import { repeat } from "../repeat";
import { applySpread, normalizeInsertedText, setAttribute, setAttributeNS, setBoolAttribute, setClass, setInnerHTML, setProperty, setStyle, setTextContent, setTextDataValue, toggleClass, type HtmlClassValue } from "./properties";
import { HtmlSession } from "./session";
import { attachChild, clearChildren, createElement, createMarker, createRange, createText, detach, insertBefore, type HtmlElement, type HtmlMarker, type HtmlNamespaceKey, type HtmlNode, type HtmlParent, type HtmlRange, type HtmlText } from "./tree";

export { renderEffect as hRenderEffect } from "@rezejs/signals/render";

const instances = new WeakMap<HtmlNode, Instance>();
const elementTypes = new WeakMap<Site, Map<string, (props: Record<string, unknown>) => HtmlElement>>();

export function htmlSession(): HtmlSession {
  const session = currentExecution();
  if (!(session instanceof HtmlSession)) throw new HydrationError("HTML helpers require an active HTML session");
  return session;
}

export function beginAwaitOperand(handle: ContinuationHandle, site: Site): void {
  htmlSession().beginOperand(handle, site);
}

export function rejectAwaitOperand<T>(handle: ContinuationHandle, error: T): T {
  const value = handle.reject(error);
  const session = currentExecution();
  if (session instanceof HtmlSession) session.rejectOperand(handle, value);
  return value;
}

function attachMoved(parent: HtmlParent, node: HtmlNode): void {
  for (let ancestor: HtmlParent | undefined = parent; ancestor !== undefined; ancestor = ancestor.parent) {
    if (ancestor === node) throw new HydrationError("cyclic HTML insertion");
  }
  detach(node);
  attachChild(parent, node);
}

function appendValue(parent: HtmlParent, value: unknown, site?: Site): void {
  while (typeof value === "function") value = (value as () => unknown)();
  if (Array.isArray(value)) {
    for (const item of value) appendValue(parent, item, site);
  } else if (value !== null && typeof value === "object" && "kind" in value && "meta" in value) {
    const node = value as HtmlNode;
    if (node.kind !== "element" && node.kind !== "text" && node.kind !== "marker" && node.kind !== "range") {
      throw new HydrationError("unsupported HTML insertion record", site);
    }
    attachMoved(parent, node);
  } else {
    const text = normalizeInsertedText(value, site);
    if (text !== undefined) attachChild(parent, createText(text));
  }
}

function bindValue(parent: HtmlParent, value: unknown, session: HtmlSession, instance: Instance, site?: Site, place?: () => void): void {
  renderEffect(() => session.instances.run(instance, () => {
    clearChildren(parent);
    appendValue(parent, value, site);
    place?.();
  }));
}

function managedRange(kind: RangeKind, role: string, site: Site, build: () => unknown, place?: (range: HtmlRange) => void): HtmlRange {
  const session = htmlSession();
  const instance = session.instances.reserve(role, site);
  session.instances.own(instance);
  const range = createRange(instance.id, { site });
  instances.set(range, instance);
  session.ranges.set(range.token, { ownerId: instance.id, range: kind });
  session.instances.run(instance, () => bindValue(range, untrack(build), session, instance, site, place === undefined ? undefined : () => place(range)));
  return range;
}

export function hMount(build: () => unknown): HtmlRange {
  const session = htmlSession();
  return session.instances.run(session.instances.root, () => root(() => {
    const range = createRange("0");
    instances.set(range, session.instances.root);
    session.ranges.set("0", { ownerId: "0", range: "fragment" });
    bindValue(range, untrack(build), session, session.instances.root);
    return range;
  }));
}

function staticMeta(root: HtmlElement, index: number): { staticIndex: number; site: unknown } {
  return { staticIndex: index, site: root.meta.site };
}

export function hRoot(tag: string, ns: HtmlNamespaceKey, site: Site): HtmlElement {
  const session = htmlSession();
  const instance = session.instances.reserve("e", site);
  session.instances.own(instance);
  const node = createElement(tag, ns, { staticIndex: 0, site, token: instance.id });
  instances.set(node, instance);
  return node;
}

export function hElement(root: HtmlElement, index: number, tag: string, ns: HtmlNamespaceKey): HtmlElement {
  const node = createElement(tag, ns, staticMeta(root, index));
  instances.set(node, instances.get(root)!);
  return node;
}

export function hText(root: HtmlElement, index: number, text: string): HtmlText {
  const node = createText(text, staticMeta(root, index));
  instances.set(node, instances.get(root)!);
  return node;
}

export function hMarker(root: HtmlElement, index: number): HtmlMarker {
  const node = createMarker(staticMeta(root, index));
  instances.set(node, instances.get(root)!);
  return node;
}

export function hAttach(parent: HtmlParent, child: HtmlNode): void {
  attachChild(parent, child);
}

export function hSetAttr(node: HtmlElement, site: Site, name: string, value: unknown): void {
  setAttribute(node, name, value, site);
}

export function hSetAttrNS(node: HtmlElement, site: Site, namespace: string, name: string, value: unknown): void {
  setAttributeNS(node, namespace, name, value, site);
}

export function hSetBool(node: HtmlElement, site: Site, name: string, value: unknown): void {
  setBoolAttribute(node, name, value, site);
}

export function hSetProp(node: HtmlElement, site: Site, name: string, value: unknown): void {
  setProperty(node, name, value, site);
}

export function hSetInnerHTML(node: HtmlElement, site: Site, value: unknown): void {
  setInnerHTML(node, value, site);
}

export function hSetClass(node: HtmlElement, site: Site, value: HtmlClassValue): void {
  setClass(node, value, site);
}

export function hSetToggle(node: HtmlElement, _site: Site, token: string, value: unknown, previous?: unknown): boolean {
  return toggleClass(node, token, value, previous);
}

export function hSetStyle(node: HtmlElement, site: Site, value: unknown, previous?: unknown): unknown {
  return setStyle(node, value, previous, site);
}

export function hSetText(node: HtmlElement | HtmlText, site: Site, value: unknown): void {
  if (node.kind === "text") setTextDataValue(node, value, site);
  else setTextContent(node, value, site);
}

function insertion(parent: HtmlParent, value: unknown, site: Site, slot: number, anchor: HtmlNode | null | undefined): void {
  const session = htmlSession();
  const owner = instances.get(parent) ?? session.instances.current();
  session.instances.run(owner, () => {
    managedRange("insertion", slot < 0 ? "spread" : `i${slot.toString(36)}`, site, () => value, (range) => {
      if (anchor === undefined) {
        if (parent.kind === "element") parent.innerHTML = undefined;
        if (parent.children.length === 1 && parent.children[0] === range) return;
        detach(range);
        clearChildren(parent);
        attachChild(parent, range);
      } else if (range.parent !== parent) {
        detach(range);
        if (anchor === null) attachChild(parent, range);
        else insertBefore(parent, range, anchor);
      }
    });
  });
}

export function hInsert(parent: HtmlParent, value: unknown, site: Site, slot: number, anchor?: HtmlNode): void {
  insertion(parent, value, site, slot, anchor);
}

export function hAppend(parent: HtmlParent, value: unknown, site: Site, slot: number): void {
  insertion(parent, value, site, slot, null);
}

export function hSpread(node: HtmlElement, site: Site, props: Record<string, unknown> = {}, isSvg = false, hasChildren = false): void {
  if (!hasChildren) insertion(node, () => props.children, site, -1, undefined);
  const previous = new Map<string, unknown>();
  renderEffect(() => {
    applySpread(node, props, isSvg, site, previous);
  });
}

export function hComponent<P>(component: (props: P) => unknown, props: P, site: Site): HtmlRange {
  return managedRange("component", "c", site, () => untrack(component, props));
}

export function hFragment(site: Site, build: () => unknown): HtmlRange {
  return managedRange("fragment", "f", site, build);
}

export function hShow<T>(site: Site, when: () => T, child: (value: () => T) => JSX.Element, fallback?: () => JSX.Element): HtmlRange {
  return managedRange("branch", "s", site, () => branch(when,
    (value) => managedRange("branch", "b1", site, () => child(value)) as unknown as JSX.Element,
    fallback === undefined ? undefined : () => managedRange("branch", "b0", site, fallback) as unknown as JSX.Element));
}

export function hChoose(site: Site, whens: readonly (() => unknown)[], children: readonly ((value: () => unknown) => JSX.Element)[], fallback?: () => JSX.Element): HtmlRange {
  return managedRange("branch", "w", site, () => choose(whens,
    children.map((child, index) => (value) => managedRange("branch", `b${index.toString(36)}`, site, () => child(value)) as unknown as JSX.Element),
    fallback === undefined ? undefined : () => managedRange("branch", "fallback", site, fallback) as unknown as JSX.Element));
}

export function hList<T>(site: Site, each: () => readonly T[] | null | undefined | false, map: (item: never, index: never) => JSX.Element, fallback?: () => JSX.Element, keyed?: boolean | ((item: T) => unknown)): HtmlRange {
  return managedRange("list", "l", site, () => {
    const row = (item: never, index: never): JSX.Element => managedRange("row", "row", site, () => map(item, index)) as unknown as JSX.Element;
    Object.defineProperty(row, "length", { value: map.length });
    const makeList = list as (each: () => readonly T[] | null | undefined | false, map: typeof row, fallback: (() => JSX.Element) | undefined, keyed: boolean | ((item: T) => unknown) | undefined) => () => JSX.Element;
    return makeList(each, row, fallback === undefined ? undefined : () => managedRange("branch", "fallback", site, fallback) as unknown as JSX.Element, keyed);
  });
}

export function hRepeat(site: Site, count: () => number, map: (index: number) => JSX.Element, fallback?: () => JSX.Element): HtmlRange {
  return managedRange("list", "repeat", site, () => repeat(count,
    (index) => managedRange("row", "row", site, () => map(index)) as unknown as JSX.Element,
    fallback === undefined ? undefined : () => managedRange("branch", "fallback", site, fallback) as unknown as JSX.Element));
}

export function hRows(site: Site, count: number, map: (index: number) => unknown): HtmlRange {
  return managedRange("list", "rows", site, () => {
    const rows: HtmlRange[] = [];
    for (let index = 0; index < count; index += 1) rows.push(managedRange("row", "row", site, () => map(index)));
    return rows;
  });
}

export function hLoading(site: Site, child: () => JSX.Element, fallback?: () => JSX.Element): HtmlRange {
  return managedRange("branch", "loading", site, () => loading(
    () => managedRange("branch", "content", site, child) as unknown as JSX.Element,
    fallback === undefined ? undefined : () => managedRange("branch", "fallback", site, fallback) as unknown as JSX.Element));
}

export function hErrored(site: Site, child: () => JSX.Element, fallback?: (error: unknown, reset: () => void) => JSX.Element): HtmlRange {
  return managedRange("branch", "errored", site, () => errored(
    () => managedRange("branch", "content", site, child) as unknown as JSX.Element,
    fallback === undefined ? undefined : (error, reset) => managedRange("branch", "fallback", site, () => fallback(error, reset)) as unknown as JSX.Element));
}

export function hAsyncComponent<V extends unknown[], R>(site: Site, load: (context: AsyncContext) => PromiseLike<V>, body: (values: () => V) => R): HtmlRange {
  return managedRange("async", "async", site, () => {
    const step = internalAsyncComputed(load);
    const values = (): V => step.value()!;
    const isLoaded = computed(() => step.value() !== undefined);
    const view = computed(() => isLoaded() ? untrack(body, values) : undefined);
    return computed(() => {
      const current = view();
      const error = step.error();
      if (error !== undefined) throw error;
      return current;
    });
  });
}

export function hDynamic<P>(site: Site, source: () => ((props: P) => unknown) | null | undefined | false): (props: P) => unknown {
  return (props) => {
    const type = computed(source);
    return () => {
      const component = type();
      return component ? hComponent(component, props, site) : undefined;
    };
  };
}

export function hElementType(site: Site, tag: string, namespace: HtmlNamespaceKey = ""): (props: Record<string, unknown>) => HtmlElement {
  let cache = elementTypes.get(site);
  if (cache === undefined) elementTypes.set(site, cache = new Map());
  const key = `${namespace}:${tag}`;
  let component = cache.get(key);
  if (component === undefined) {
    component = (props) => {
      const node = hRoot(tag, namespace, site);
      hSpread(node, site, props, namespace !== "");
      return node;
    };
    cache.set(key, component);
  }
  return component;
}

export function hDynamicElement(site: Site, source: () => string | ((props: Record<string, unknown>) => unknown) | null | undefined | false): (props: Record<string, unknown>) => unknown {
  return hDynamic(site, () => {
    const type = source();
    if (typeof type !== "string") return type;
    if (!type) return undefined;
    return hElementType(site, type, Object.hasOwn(SVGElements, type) ? "svg" : type === "math" ? "math" : "");
  });
}

export function hPortal(site: Site, child: () => unknown, mount?: () => unknown): HtmlRange {
  return managedRange("portal", "portal", site, () => {
    const session = htmlSession();
    const content = managedRange("portal", "content", site, child);
    const instance = instances.get(content)!;
    const placement = mount === undefined ? "body" : "inert";
    session.ranges.set(content.token, { ownerId: instance.id, range: "portal", placement });
    session.portals.push({ node: content, instance, placement });
    return undefined;
  });
}

export function hIsland<P>(site: Site, trigger: string, load: () => ((props: P) => unknown) | PromiseLike<(props: P) => unknown>, props: P, fallback?: () => unknown): HtmlRange {
  return managedRange("island", "island", site, () => {
    if (trigger !== "eager") {
      if (trigger !== "visible" && trigger !== "interaction") return fallback?.();
      const host = hRoot("span", "", site);
      setAttribute(host, "data-island", trigger, site);
      if (fallback === undefined) setAttribute(host, "style", "display:block;min-width:1px;min-height:1px", site);
      else hAppend(host, fallback, site, 0);
      return host;
    }
    const loaded = load();
    if (typeof loaded === "function") return hComponent(loaded, props, site);
    return hAsyncComponent(site, () => Promise.resolve(loaded).then((component) => [component]), (values) => hComponent(values()[0]!, props, site));
  });
}
