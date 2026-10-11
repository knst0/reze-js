import { getOwner, onCleanup } from "@rezejs/signals";

/**
 * Marks head tags the router or `<Head>` manages; unmarked tags are template baseline and never touched. Keyed tags
 * carry it bare, unkeyed ones carry the stream token as its value. The compiler emits the same key shape for `<Head>`
 * tags, so route metadata and `<Head>` dedupe against each other.
 */
export const HeadManagedAttribute = "data-rz-head";

/** Carries the key or stream token of a streamed head tag on its `<template data-rz-head>` unit. */
export const HeadKeyAttribute = "data-rz-head-key";

export function headScriptKey(src: string): string {
  return `script:${src}`;
}

export function headLinkKey(rel: string, href: string): string {
  return `link:${rel}\n${href}`;
}

export function headMetaNameKey(name: string): string {
  return `meta:name:${name}`;
}

export function headMetaPropertyKey(property: string): string {
  return `meta:property:${property}`;
}

export function headElementKey(element: Element): string | undefined {
  if (element.tagName === "SCRIPT") {
    const src = element.getAttribute("src");
    return src === null ? undefined : headScriptKey(src);
  }
  if (element.tagName === "LINK") {
    const rel = element.getAttribute("rel");
    const href = element.getAttribute("href");
    return rel === null || href === null ? undefined : headLinkKey(rel, href);
  }
  if (element.tagName === "META") {
    const name = element.getAttribute("name");
    if (name !== null) return headMetaNameKey(name);
    const property = element.getAttribute("property");
    return property === null ? undefined : headMetaPropertyKey(property);
  }
  return undefined;
}

export function headKeyKind(key: string): "script" | "link" | "meta" | undefined {
  if (key.startsWith("script:")) return "script";
  if (key.startsWith("link:")) return "link";
  if (key.startsWith("meta:")) return "meta";
  return undefined;
}

export function headToken(element: Element): string | undefined {
  const value = element.getAttribute(HeadManagedAttribute);
  return value === null || value === "" ? undefined : value;
}

interface HeadEntry {
  element: Element;
  owners: Set<object>;
  persistent: boolean;
}

const entries = new Map<string, HeadEntry>();
let ownedCount = 0;

function findLive(key: string): HeadEntry | undefined {
  const entry = entries.get(key);
  if (entry === undefined) return undefined;
  if (entry.element.isConnected) return entry;
  entries.delete(key);
  return undefined;
}

function entryForElement(element: Element): HeadEntry | undefined {
  for (const entry of entries.values()) {
    if (entry.element === element) return entry;
  }
  return undefined;
}

function adoptElement(id: string): Element | undefined {
  if (headKeyKind(id) === undefined) {
    return document.head.querySelector(`[${HeadManagedAttribute}="${id}"]`) ?? undefined;
  }
  for (const element of document.head.querySelectorAll(`[${HeadManagedAttribute}]`)) {
    if (headElementKey(element) === id) return element;
  }
  return undefined;
}

function claim(element: Element, id: string, owner: object): void {
  const owned = entryForElement(element);
  if (owned !== undefined) {
    owned.owners.add(owner);
    return;
  }
  entries.set(id, { element, owners: new Set([owner]), persistent: true });
}

function mountScript(key: string, build: () => Element, owner: object): void {
  const entry = findLive(key);
  if (entry !== undefined) {
    entry.owners.add(owner);
    return;
  }
  const adopted = adoptElement(key);
  if (adopted !== undefined) {
    claim(adopted, key, owner);
    return;
  }
  const created = build();
  created.setAttribute(HeadManagedAttribute, "");
  document.head.append(created);
  entries.set(key, { element: created, owners: new Set([owner]), persistent: false });
}

function replaceHeadTag(key: string, build: () => Element, owner: object): void {
  const entry = findLive(key);
  const created = build();
  created.setAttribute(HeadManagedAttribute, "");
  if (entry !== undefined) {
    entry.element.replaceWith(created);
    entry.element = created;
    entry.owners.add(owner);
    entry.persistent = false;
    return;
  }
  const adopted = adoptElement(key);
  if (adopted !== undefined) {
    adopted.replaceWith(created);
  } else {
    document.head.append(created);
  }
  entries.set(key, { element: created, owners: new Set([owner]), persistent: false });
}

function mountUnkeyed(build: () => Element, owner: object): void {
  const created = build();
  if (created.tagName === "SCRIPT" && created.getAttribute("src") === null) {
    for (const element of document.head.querySelectorAll(`script[${HeadManagedAttribute}]`)) {
      if (
        element.getAttribute("src") === null &&
        element.getAttribute("type") === created.getAttribute("type") &&
        element.textContent === created.textContent
      ) {
        claim(element, headToken(element) ?? `owned:${ownedCount++}`, owner);
        return;
      }
    }
  }
  created.setAttribute(HeadManagedAttribute, "");
  document.head.append(created);
  entries.set(`owned:${ownedCount++}`, { element: created, owners: new Set([owner]), persistent: false });
}

/**
 * Runtime of `<Head>`: mounts each tag into the document head and releases it when the owner disposes. Scripts adopt
 * the live tag so they execute once; links and metas replace it so they stay live; unkeyed scripts adopt on matching
 * content. Disposing removes only tags no live owner claims.
 */
export function head(items: [key: string | undefined, build: () => Element][]): void {
  const owner = getOwner() ?? {};
  for (const [key, build] of items) {
    if (key === undefined) mountUnkeyed(build, owner);
    else if (headKeyKind(key) === "script") mountScript(key, build, owner);
    else replaceHeadTag(key, build, owner);
  }
  onCleanup(() => releaseHeadOwner(owner));
}

export function releaseHeadOwner(owner: object): void {
  for (const [key, entry] of entries) {
    if (!entry.owners.delete(owner)) continue;
    if (entry.owners.size === 0 && !entry.persistent) {
      entry.element.remove();
      entries.delete(key);
    }
  }
}

export interface RouteHeadWant {
  matches(element: Element): boolean;
  create(): Element;
}

export function reconcileRouteHead(wanted: Map<string, RouteHeadWant>, routeOwner: object): void {
  for (const [key, want] of wanted) {
    const entry = findLive(key);
    if (entry !== undefined && want.matches(entry.element)) {
      entry.owners.add(routeOwner);
      continue;
    }
    const adopted = entry === undefined ? adoptElement(key) : undefined;
    if (adopted !== undefined && want.matches(adopted)) {
      claim(adopted, key, routeOwner);
      continue;
    }
    (entry?.element ?? adopted)?.remove();
    if (entry !== undefined) entries.delete(key);
    const created = want.create();
    created.setAttribute(HeadManagedAttribute, "");
    document.head.append(created);
    entries.set(key, { element: created, owners: new Set([routeOwner]), persistent: false });
  }
  for (const [key, entry] of entries) {
    if (wanted.has(key)) continue;
    if (!entry.owners.delete(routeOwner)) continue;
    if (entry.owners.size === 0 && !entry.persistent) {
      entry.element.remove();
      entries.delete(key);
    }
  }
}

export function dropHeadElement(element: Element): void {
  for (const [key, entry] of entries) {
    if (entry.element === element) entries.delete(key);
  }
}

export function registerStaticHead(element: Element): void {
  const id = headElementKey(element) ?? headToken(element);
  if (id !== undefined) entries.set(id, { element, owners: new Set(), persistent: true });
}

/**
 * Applies one streamed head unit: replaces the live tag the unit names, or appends it when absent. An empty unit
 * removes the live tag. Streamed tags are page content, so they persist without owners.
 */
export function mergeStreamedHead(target: Document, unit: HTMLTemplateElement): void {
  const id = unit.getAttribute(HeadKeyAttribute);
  const nodes = [...unit.content.children];
  const live = id === null ? undefined : (findLive(id)?.element ?? adoptElement(id));
  if (nodes.length === 0) {
    live?.remove();
    if (id !== null) entries.delete(id);
    return;
  }
  const imported = nodes.map((node) => target.importNode(node, true));
  if (live !== undefined) {
    live.replaceWith(...imported);
    dropHeadElement(live);
  } else {
    for (const node of imported) target.head.append(node);
  }
  for (const node of imported) registerStaticHead(node);
}
