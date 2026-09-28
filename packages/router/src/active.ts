import { setAttribute } from "@rezejs/dom";
import { computed, runWithOwner, selector, useContext } from "@rezejs/signals";
import { renderEffect } from "@rezejs/signals/render";

import { useRouter } from "./hooks";
import { pathKey } from "./match";
import { pathnameOf, resolveHref, RouterContext, type LinkSelectors, type RouterState } from "./navigation";
import type { Href } from "./types";

const Current = 1;
const Active = 2;
const Pending = 4;
const Scheme = /^[a-z][a-z\d+.-]*:/i;

export interface LinkState {
  /** The link's pathname is the current one: `aria-current="page"`. */
  readonly current: () => boolean;
  /** The link's pathname is the current one or a segment-prefix of it (`/` only exactly): `data-active`. */
  readonly active: () => boolean;
  /** A navigation to exactly the link's pathname waits for route modules: `data-pending`. */
  readonly pending: () => boolean;
}

function selectorsOf(state: RouterState): LinkSelectors {
  return (state.links ??= runWithOwner(state.owner, () => {
    const currentKey = computed(() => pathKey(state.location().pathname));
    return { currentKey, isCurrent: selector(currentKey), isPrefixAt: [], isPending: selector(state.pendingKey) };
  }));
}

function prefixOf(key: string, depth: number): string | undefined {
  let end = 0;
  for (let i = 0; i < depth; i++) {
    end = key.indexOf("/", end + 1);
    if (end < 0) return undefined;
  }
  return key.slice(0, end);
}

function isPrefixAt(state: RouterState, links: LinkSelectors, depth: number): (key: string) => boolean {
  return (links.isPrefixAt[depth] ??= runWithOwner(state.owner, () => selector(() => prefixOf(links.currentKey(), depth))));
}

function linkKey(state: RouterState, href: string | null | undefined): string | undefined {
  if (!href || href[0] === "?" || (href[0] === "#" && href[1] !== "/")) return undefined;
  if (href[0] !== "/" && !Scheme.test(href)) state.location();
  const path = resolveHref(state, href);
  return path === undefined ? undefined : pathKey(pathnameOf(path));
}

function linkFlags(state: RouterState, key: string | undefined): number {
  if (key === undefined) return 0;
  const links = selectorsOf(state);
  let flags = 0;
  if (links.isCurrent(key)) {
    flags = Current | Active;
  } else if (key !== "/") {
    let depth = 0;
    for (let at = key.indexOf("/"); at >= 0; at = key.indexOf("/", at + 1)) depth++;
    if (isPrefixAt(state, links, depth)(key)) flags = Active;
  }
  return links.isPending(key) ? flags | Pending : flags;
}

function applyFlags(el: Element, flags: number, previous: number): number {
  const changed = flags ^ previous;
  if (changed & Current) setAttribute(el, "aria-current", flags & Current ? "page" : null);
  if (changed & Active) setAttribute(el, "data-active", flags & Active ? "" : null);
  if (changed & Pending) setAttribute(el, "data-pending", flags & Pending ? "" : null);
  return flags;
}

/** Compiler target for claimed `<a>`: binds `href` when given and keeps `aria-current`/`data-active`/`data-pending` current. */
export function link(el: Element, href?: () => string): void {
  const value = href === undefined ? undefined : computed(href);
  if (value !== undefined) renderEffect(() => setAttribute(el, "href", value()));
  const state = useContext(RouterContext);
  if (state === undefined) return;
  const staticHref = value === undefined ? el.getAttribute("href") : undefined;
  renderEffect(
    (previous: number) => applyFlags(el, linkFlags(state, linkKey(state, value === undefined ? staticHref : value())), previous),
    0,
  );
}

/** The state `<a href>` gets as attributes, for links the compiler cannot claim (spread props, library components). */
export function useLinkState(href: () => Href): LinkState {
  const state = useRouter();
  const flags = computed(() => linkFlags(state, linkKey(state, href())));
  return {
    current: () => (flags() & Current) !== 0,
    active: () => (flags() & Active) !== 0,
    pending: () => (flags() & Pending) !== 0,
  };
}
