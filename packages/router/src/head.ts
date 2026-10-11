import {
  HeadManagedAttribute,
  dropHeadElement,
  headElementKey,
  headLinkKey,
  headMetaNameKey,
  headMetaPropertyKey,
  headScriptKey,
  reconcileRouteHead,
  registerStaticHead,
} from "@rezejs/dom/internal/client";

import type { PageHeadLink, PageHeadMeta, PageHeadScript, PageMetadata } from "./types";

export type HeadWant =
  | { readonly kind: "script"; readonly key: string; readonly spec: PageHeadScript }
  | { readonly kind: "link"; readonly key: string; readonly spec: PageHeadLink }
  | { readonly kind: "meta"; readonly key: string; readonly spec: PageHeadMeta };

export function headWants(metadata: PageMetadata): HeadWant[] {
  const wants: HeadWant[] = [];
  for (const spec of metadata.scripts ?? []) wants.push({ kind: "script", key: headScriptKey(spec.src), spec });
  for (const spec of metadata.links ?? []) wants.push({ kind: "link", key: headLinkKey(spec.rel, spec.href), spec });
  for (const spec of metadata.metas ?? []) {
    wants.push({
      kind: "meta",
      key: spec.name === undefined ? headMetaPropertyKey(spec.property) : headMetaNameKey(spec.name),
      spec,
    });
  }
  return wants;
}

function setAttribute(element: Element, name: string, value: string | undefined): void {
  if (value !== undefined) element.setAttribute(name, value);
}

export function applyHeadSpec(element: Element, want: HeadWant): void {
  if (want.kind === "script") {
    const spec = want.spec;
    element.setAttribute("src", spec.src);
    if (spec.async === true) element.setAttribute("async", "");
    if (spec.defer === true) element.setAttribute("defer", "");
    setAttribute(element, "type", spec.type);
    setAttribute(element, "crossorigin", spec.crossorigin);
    setAttribute(element, "integrity", spec.integrity);
    (element as HTMLScriptElement).async = spec.async ?? false;
  } else if (want.kind === "link") {
    const spec = want.spec;
    element.setAttribute("rel", spec.rel);
    element.setAttribute("href", spec.href);
    setAttribute(element, "as", spec.as);
    setAttribute(element, "type", spec.type);
    setAttribute(element, "media", spec.media);
    setAttribute(element, "hreflang", spec.hreflang);
    setAttribute(element, "crossorigin", spec.crossorigin);
  } else {
    const spec = want.spec;
    if (spec.name !== undefined) element.setAttribute("name", spec.name);
    else element.setAttribute("property", spec.property);
    element.setAttribute("content", spec.content);
  }
  element.setAttribute(HeadManagedAttribute, "");
}

function specEntries(want: HeadWant): (readonly [string, string | undefined])[] {
  if (want.kind === "script") {
    const spec = want.spec;
    return [
      ["src", spec.src],
      ["async", spec.async === true ? "" : undefined],
      ["defer", spec.defer === true ? "" : undefined],
      ["type", spec.type],
      ["crossorigin", spec.crossorigin],
      ["integrity", spec.integrity],
    ];
  }
  if (want.kind === "link") {
    const spec = want.spec;
    return [
      ["rel", spec.rel],
      ["href", spec.href],
      ["as", spec.as],
      ["type", spec.type],
      ["media", spec.media],
      ["hreflang", spec.hreflang],
      ["crossorigin", spec.crossorigin],
    ];
  }
  const spec = want.spec;
  return spec.name === undefined
    ? [
        ["property", spec.property],
        ["content", spec.content],
      ]
    : [
        ["name", spec.name],
        ["content", spec.content],
      ];
}

function elementAttributes(element: Element): Map<string, string> {
  const out = new Map<string, string>();
  for (const { name, value } of element.attributes) {
    if (name !== HeadManagedAttribute) out.set(name, value);
  }
  return out;
}

function specAttributes(want: HeadWant): Map<string, string> {
  const out = new Map<string, string>();
  for (const [name, value] of specEntries(want)) {
    if (value !== undefined) out.set(name, value);
  }
  return out;
}

function attributesEqual(left: Map<string, string>, right: Map<string, string>): boolean {
  if (left.size !== right.size) return false;
  for (const [name, value] of left) {
    if (right.get(name) !== value) return false;
  }
  return true;
}

export function headSpecMatches(element: Element, want: HeadWant): boolean {
  return attributesEqual(elementAttributes(element), specAttributes(want));
}

export function headElementsMatch(left: Element, right: Element): boolean {
  if (left.tagName !== right.tagName) return false;
  return attributesEqual(elementAttributes(left), elementAttributes(right));
}

export function applyRouteHead(wants: readonly HeadWant[], routeOwner: object): void {
  const wanted = new Map(
    wants.map((want) => [
      want.key,
      {
        matches: (element: Element) => headSpecMatches(element, want),
        create: () => {
          const created = document.createElement(want.kind);
          applyHeadSpec(created, want);
          return created;
        },
      },
    ]),
  );
  reconcileRouteHead(wanted, routeOwner);
}

function cloneHeadScript(source: Element): HTMLScriptElement {
  const created = document.createElement("script");
  for (const { name, value } of source.attributes) {
    if (name !== HeadManagedAttribute) created.setAttribute(name, value);
  }
  created.async = source.hasAttribute("async");
  created.setAttribute(HeadManagedAttribute, "");
  return created;
}

/**
 * Reconciles the live head with a fetched document: keyed tags diff by key with scripts recreated so they execute,
 * unkeyed tags replace wholesale because stream tokens never match across pages.
 */
export function syncManagedHead(live: Document, incoming: Document): void {
  const incomingByKey = new Map<string, Element>();
  const incomingUnkeyed: Element[] = [];
  for (const tag of incoming.head.querySelectorAll(`[${HeadManagedAttribute}]`)) {
    const key = headElementKey(tag);
    if (key === undefined) incomingUnkeyed.push(tag);
    else incomingByKey.set(key, tag);
  }
  const kept = new Map<string, Element>();
  for (const current of live.head.querySelectorAll(`[${HeadManagedAttribute}]`)) {
    const key = headElementKey(current);
    if (key === undefined) {
      current.remove();
      dropHeadElement(current);
      continue;
    }
    const next = incomingByKey.get(key);
    if (next === undefined || !headElementsMatch(current, next)) {
      current.remove();
      dropHeadElement(current);
    } else {
      kept.set(key, current);
    }
  }
  for (const [key, tag] of incomingByKey) {
    const existing = kept.get(key);
    if (existing !== undefined) {
      live.head.append(existing);
      continue;
    }
    const adopted = tag.tagName === "SCRIPT" && tag.hasAttribute("src") ? cloneHeadScript(tag) : live.adoptNode(tag);
    live.head.append(adopted);
    registerStaticHead(adopted);
  }
  for (const tag of incomingUnkeyed) {
    const adopted = live.adoptNode(tag);
    live.head.append(adopted);
    registerStaticHead(adopted);
  }
}
