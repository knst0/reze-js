import { getOwner, onCleanup } from "@rezejs/signals";

/**
 * Marks head tags `<Head>` and the server manage; unmarked tags are template baseline that a keyed `<Head>` tag only
 * shadows. Keyed tags carry it bare, unkeyed ones carry the stream token as its value.
 */
export const HeadManagedAttribute = "data-rz-head";

/** Carries the key or stream token of a streamed head tag on its `<template data-rz-head>` unit. */
export const HeadKeyAttribute = "data-rz-head-key";

/** Identity of a head tag; the compiler emits the same keys for `<Head>` tags and the server for document defaults. */
export function headKey(tag: string, attribute: (name: string) => string | null): string | undefined {
  if (tag === "title" || tag === "base") return tag;
  if (tag === "script") {
    const src = attribute("src");
    return src === null ? undefined : `script:${src}`;
  }
  if (tag === "link") {
    const rel = attribute("rel");
    if (rel === null) return undefined;
    if (rel.toLowerCase() === "canonical") return "link:canonical";
    const href = attribute("href");
    return href === null ? undefined : `link:${rel}\n${href}`;
  }
  if (tag !== "meta") return undefined;
  const name = attribute("name");
  if (name !== null) return `meta:name:${name}`;
  const property = attribute("property");
  if (property !== null) return `meta:property:${property}`;
  const httpEquiv = attribute("http-equiv");
  return httpEquiv === null ? undefined : `meta:http-equiv:${httpEquiv}`;
}

export function headElementKey(element: Element): string | undefined {
  return headKey(element.localName, (name) => element.getAttribute(name));
}

export function isHeadScriptKey(id: string): boolean {
  return id.startsWith("script:");
}

interface HeadClaim {
  readonly owner: object;
  readonly element: Element;
}

interface HeadSlot {
  base: Element | undefined;
  claims: HeadClaim[];
}

interface UnkeyedEntry {
  readonly owners: Set<object>;
  readonly isAdopted: boolean;
}

const slots = new Map<string, HeadSlot>();
const Lingering = {};
const unkeyed = new Map<Element, UnkeyedEntry>();

function findHeadElement(head: HTMLHeadElement, id: string): Element | undefined {
  for (const element of head.children) {
    if (element.getAttribute(HeadManagedAttribute) === id || headElementKey(element) === id) return element;
  }
  return undefined;
}

function shown(slot: HeadSlot): Element | undefined {
  return slot.claims.length === 0 ? slot.base : slot.claims[slot.claims.length - 1]!.element;
}

function show(previous: Element | undefined, next: Element | undefined): void {
  if (previous === next) return;
  if (next === undefined) previous?.remove();
  else if (previous?.isConnected === true) previous.replaceWith(next);
  else document.head.append(next);
}

function managedElement(build: () => Element): Element {
  const created = build();
  created.setAttribute(HeadManagedAttribute, "");
  return created;
}

function claimKey(key: string, build: () => Element, owner: object): void {
  let slot = slots.get(key);
  if (slot === undefined) {
    slot = { base: findHeadElement(document.head, key), claims: [] };
    slots.set(key, slot);
  }
  const previous = shown(slot);
  const element = isHeadScriptKey(key) && previous !== undefined ? previous : managedElement(build);
  slot.claims.push({ owner, element });
  show(previous, element);
}

function claimUnkeyed(build: () => Element, owner: object): void {
  const created = build();
  for (const element of document.head.querySelectorAll(`[${HeadManagedAttribute}]`)) {
    if (
      element.localName === created.localName &&
      headElementKey(element) === undefined &&
      element.getAttribute("type") === created.getAttribute("type") &&
      element.textContent === created.textContent
    ) {
      const entry = unkeyed.get(element);
      if (entry === undefined) unkeyed.set(element, { owners: new Set([owner]), isAdopted: true });
      else entry.owners.add(owner);
      return;
    }
  }
  created.setAttribute(HeadManagedAttribute, "");
  document.head.append(created);
  unkeyed.set(created, { owners: new Set([owner]), isAdopted: false });
}

/**
 * Runtime of `<Head>`: each keyed tag shadows the tag of the same key from an earlier owner or the page, and the latest
 * live owner's tag shows. Scripts adopt the shown tag so they execute once, and outlive their last owner until the next
 * microtask so an owner mounted in the same turn adopts them. Disposing the owner shows the tag it shadowed again;
 * unkeyed tags adopt a matching page tag or append and leave with their last owner.
 */
export function head(items: [key: string | undefined, build: () => Element][]): void {
  const owner = getOwner() ?? {};
  for (const [key, build] of items) {
    if (key === undefined) claimUnkeyed(build, owner);
    else claimKey(key, build, owner);
  }
  onCleanup(() => releaseHeadOwner(owner));
}

export function releaseHeadOwner(owner: object): void {
  for (const [key, slot] of slots) {
    const claims = slot.claims.filter((claim) => claim.owner !== owner);
    if (claims.length === slot.claims.length) continue;
    const previous = shown(slot);
    if (claims.length === 0 && owner !== Lingering && isHeadScriptKey(key) && previous !== slot.base) {
      slot.claims = [{ owner: Lingering, element: previous! }];
      queueMicrotask(() => releaseHeadOwner(Lingering));
      continue;
    }
    slot.claims = claims;
    show(previous, shown(slot));
    if (claims.length === 0) slots.delete(key);
  }
  for (const [element, entry] of unkeyed) {
    if (!entry.owners.delete(owner) || entry.owners.size > 0) continue;
    if (!entry.isAdopted) element.remove();
    unkeyed.delete(element);
  }
}

export function isHeadKeyClaimed(key: string): boolean {
  return slots.has(key);
}

/** Replaces the page tag each claimed key shows again once its last owner disposes; a new page brings new ones. */
export function rebaseHead(baseOf: (key: string) => Element | undefined): void {
  for (const [key, slot] of slots) slot.base = baseOf(key);
}

/**
 * Applies one streamed head unit: replaces the live tag the unit names, or appends it when absent. An empty unit
 * removes the live tag. A key a `<Head>` owner claims keeps showing the claim and takes the unit as its page tag.
 */
export function mergeStreamedHead(target: Document, unit: HTMLTemplateElement): void {
  const id = unit.getAttribute(HeadKeyAttribute) ?? "";
  const imported = [...unit.content.children].map((node) => target.importNode(node, true));
  const slot = slots.get(id);
  if (slot !== undefined) {
    slot.base = imported[0];
    return;
  }
  const live = findHeadElement(target.head, id);
  if (imported.length === 0) live?.remove();
  else if (live !== undefined) live.replaceWith(...imported);
  else target.head.append(...imported);
}
