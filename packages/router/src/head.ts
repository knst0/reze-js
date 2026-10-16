import { HeadManagedAttribute, headElementKey, isHeadKeyClaimed, rebaseHead } from "@rezejs/dom/internal/client";

function attributesOf(element: Element): Map<string, string> {
  const out = new Map<string, string>();
  for (const { name, value } of element.attributes) {
    if (name !== HeadManagedAttribute) out.set(name, value);
  }
  return out;
}

export function headElementsMatch(left: Element, right: Element): boolean {
  if (left.tagName !== right.tagName || left.textContent !== right.textContent) return false;
  const leftAttributes = attributesOf(left);
  const rightAttributes = attributesOf(right);
  if (leftAttributes.size !== rightAttributes.size) return false;
  for (const [name, value] of leftAttributes) {
    if (rightAttributes.get(name) !== value) return false;
  }
  return true;
}

function cloneHeadScript(source: Element): HTMLScriptElement {
  const created = document.createElement("script");
  for (const { name, value } of source.attributes) created.setAttribute(name, value);
  created.async = source.hasAttribute("async");
  return created;
}

function adoptHeadTag(live: Document, tag: Element): Element {
  return tag.tagName === "SCRIPT" && tag.hasAttribute("src") ? cloneHeadScript(tag) : live.adoptNode(tag);
}

/**
 * Reconciles the live head with a fetched document: keyed tags diff by key with scripts recreated so they execute,
 * unkeyed tags replace wholesale because stream tokens never match across pages. A key a live `<Head>` owner claims
 * keeps its tag and takes the fetched one as the tag it shows once the owner disposes.
 */
export function syncManagedHead(live: Document, incoming: Document): void {
  const incomingByKey = new Map<string, Element>();
  const incomingUnkeyed: Element[] = [];
  for (const tag of incoming.head.querySelectorAll(`[${HeadManagedAttribute}]`)) {
    const key = headElementKey(tag);
    if (key === undefined) incomingUnkeyed.push(tag);
    else incomingByKey.set(key, tag);
  }
  rebaseHead((key) => {
    const tag = incomingByKey.get(key);
    incomingByKey.delete(key);
    return tag === undefined ? undefined : adoptHeadTag(live, tag);
  });
  const kept = new Map<string, Element>();
  for (const current of live.head.querySelectorAll(`[${HeadManagedAttribute}]`)) {
    const key = headElementKey(current);
    if (key !== undefined && isHeadKeyClaimed(key)) continue;
    const next = key === undefined ? undefined : incomingByKey.get(key);
    if (next !== undefined && headElementsMatch(current, next)) kept.set(key!, current);
    else current.remove();
  }
  for (const [key, tag] of incomingByKey) live.head.append(kept.get(key) ?? adoptHeadTag(live, tag));
  for (const tag of incomingUnkeyed) live.head.append(live.adoptNode(tag));
}
