import { attachStream, disposeIslandsBetween, disposeIslandsIn, type BootOptions } from "@rezejs/dom/internal/client";
import { getOwner, provideContext, root } from "@rezejs/signals";

import { syncManagedHead } from "../head";
import { createBrowserHistory, routerBase, type HistoryEntry } from "../history";
import { anchorOf, anchorPath } from "../links";
import {
  commit,
  initRouterState,
  loadPositions,
  navigate,
  onPop,
  parseLocation,
  persistPositions,
  RouterStateContext,
  type ActiveMatch,
  type RouterState,
  type ScrollMode,
} from "../navigation";
import type { Location, Params } from "../types";

export interface SwapOptions {
  readonly base: string;
  readonly rootId: string;
  readonly islands?: BootOptions["islands"];
}

export interface SwapRuntime {
  readonly state: RouterState;
  wrap<T>(render: () => T): T;
  dispose(): void;
}

interface RouteSnapshot {
  readonly pathname?: string;
  readonly search?: string;
  readonly matches: readonly { readonly id: string; readonly params: Params }[];
}

const ShellMarker = "<!--rz-shell-->";
const HoverDelayMs = 20;
const PrefetchTtlMs = 10_000;

function readRoute(scope: ParentNode): RouteSnapshot {
  const text = scope.querySelector("script[data-rz-route]")?.textContent;
  return text === undefined || text === null || text === "" ? { matches: [] } : (JSON.parse(text) as RouteSnapshot);
}

function routedEntry(route: RouteSnapshot, entry: HistoryEntry): HistoryEntry {
  if (route.pathname === undefined) return entry;
  const hashAt = entry.path.indexOf("#");
  return { ...entry, path: route.pathname + (route.search ?? "") + (hashAt < 0 ? "" : entry.path.slice(hashAt)) };
}

function stubMatches(route: RouteSnapshot, path: string): ActiveMatch[] {
  return route.matches.map(
    (match) =>
      ({
        route: { id: match.id },
        path,
        params: match.params,
        data: undefined,
        hasData: false,
        error: undefined,
        info: undefined,
      }) as unknown as ActiveMatch,
  );
}

function commentRange(scope: Document, data: string): { start: Comment; end: Comment } | undefined {
  const walker = scope.createTreeWalker(scope.documentElement, NodeFilter.SHOW_COMMENT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if ((node as Comment).data !== data) continue;
    const closing = `/${data}`;
    for (let next = node.nextSibling; next !== null; next = next.nextSibling) {
      if (next.nodeType === 8 && (next as Comment).data === closing) return { start: node as Comment, end: next as Comment };
    }
  }
  return undefined;
}

function replaceBetween(live: Document, incoming: Document, data: string): void {
  const target = commentRange(live, data);
  const source = commentRange(incoming, data);
  if (target === undefined || source === undefined) return;
  while (target.start.nextSibling !== target.end) target.start.nextSibling!.remove();
  for (let node = source.start.nextSibling; node !== null && node !== source.end; node = source.start.nextSibling) {
    target.end.before(live.adoptNode(node));
  }
}

interface LayoutRange {
  readonly key: string;
  readonly start: Comment;
  readonly end: Comment;
}

const LayoutPrefix = "l:";

function isBetween(node: Node, start: Node, end: Node): boolean {
  return (
    (start.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 &&
    (end.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_PRECEDING) !== 0
  );
}

function commentsIn(scope: Node): Comment[] {
  const out: Comment[] = [];
  const walker = scope.ownerDocument!.createTreeWalker(scope, NodeFilter.SHOW_COMMENT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) out.push(node as Comment);
  return out;
}

function tokenOf(comment: Comment): string {
  return comment.data.startsWith("/rz:") ? comment.data.slice(4) : comment.data.slice(3);
}

function tokenComments(scope: Document): Comment[] {
  return commentsIn(scope.documentElement).filter(
    (comment) => comment.data.startsWith("rz:") || comment.data.startsWith("/rz:"),
  );
}

/** Route levels in document order, outermost first; each carries the key its marker range was rendered with. */
function layoutRanges(scope: Document): LayoutRange[] {
  const out: LayoutRange[] = [];
  const open: { key: string; start: Comment }[] = [];
  for (const comment of tokenComments(scope)) {
    const token = tokenOf(comment);
    if (comment.data.startsWith("/rz:")) {
      if (token.startsWith(LayoutPrefix) && open.at(-1)?.key === token) out.push({ key: token, start: open.pop()!.start, end: comment });
    } else if (token.startsWith(LayoutPrefix)) {
      open.push({ key: token, start: comment });
    }
  }
  return out.sort((left, right) =>
    (left.start.compareDocumentPosition(right.start) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 ? -1 : 1,
  );
}

function elementsIn(scope: Document, start: Node, end: Node): Element[] {
  const out: Element[] = [];
  const walker = scope.createTreeWalker(scope.documentElement, NodeFilter.SHOW_ELEMENT);
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    if (isBetween(node, start, end)) out.push(node as Element);
  }
  return out;
}

/** `[persist]` elements that are not inside another one: nested regions travel with their owner. */
function persistRegions(elements: readonly Element[]): Element[] {
  return elements.filter((element) => element.hasAttribute("persist") && element.parentElement?.closest("[persist]") === null);
}

function detachPersisted(elements: readonly Element[], kept: Map<string, Element>): void {
  for (const element of persistRegions(elements)) {
    const key = element.getAttribute("persist")!;
    if (kept.has(key)) continue;
    kept.set(key, element);
    element.remove();
  }
}

function restorePersisted(elements: readonly Element[], kept: Map<string, Element>): void {
  for (const element of persistRegions(elements)) {
    const key = element.getAttribute("persist")!;
    const live = kept.get(key);
    if (live === undefined) continue;
    kept.delete(key);
    element.replaceWith(live);
  }
}

function rangeText(scope: Document, range: { start: Comment; end: Comment }): string {
  let out = "";
  for (let node = range.start.nextSibling; node !== null && node !== range.end; node = node.nextSibling) {
    out += node.nodeType === 1 ? (node as Element).outerHTML : (node.textContent ?? "");
  }
  return out;
}

function replacePortals(incoming: Document, always: boolean): void {
  const target = commentRange(document, "rz:p");
  const source = commentRange(incoming, "rz:p");
  if (target === undefined || source === undefined) return;
  if (!always && rangeText(document, target) === rangeText(incoming, source)) return;
  replaceBetween(document, incoming, "rz:p");
}

function syncRouteScript(incoming: Document): void {
  const next = incoming.querySelector("script[data-rz-route]");
  if (next === null) return;
  const live = document.querySelector("script[data-rz-route]");
  if (live === null) document.head.append(document.adoptNode(next));
  else live.textContent = next.textContent;
}

function awaitLoaded(link: HTMLLinkElement): Promise<void> {
  return new Promise((resolve) => {
    link.addEventListener("load", () => resolve(), { once: true });
    link.addEventListener("error", () => resolve(), { once: true });
  });
}

async function syncHead(incoming: Document): Promise<void> {
  syncManagedHead(document, incoming);
  const known = new Set([...document.querySelectorAll<HTMLLinkElement>('link[rel="stylesheet"]')].map((link) => link.href));
  const pending: Promise<void>[] = [];
  for (const link of incoming.querySelectorAll<HTMLLinkElement>('link[rel="stylesheet"]')) {
    if (known.has(new URL(link.getAttribute("href")!, document.baseURI).href)) continue;
    const adopted = document.adoptNode(link);
    document.head.append(adopted);
    pending.push(awaitLoaded(adopted));
  }
  await Promise.all(pending);
}

function servedBase(base: string, route: RouteSnapshot): string {
  if (routerBase(base) !== "" || route.pathname === undefined) return base;
  const served = window.location.pathname;
  return served.length > route.pathname.length && served.endsWith(route.pathname)
    ? served.slice(0, served.length - route.pathname.length)
    : base;
}

export function installSwapNavigation(options: SwapOptions): SwapRuntime {
  const history = createBrowserHistory(servedBase(options.base, readRoute(document)));
  const currentBuild = document.querySelector('meta[name="reze-build"]')?.getAttribute("content");
  const prefetched = new Map<string, { readonly at: number; readonly response: Promise<Response> }>();

  const fetchPage = (url: string): Promise<Response> => {
    const hit = prefetched.get(url);
    prefetched.delete(url);
    if (hit !== undefined && Date.now() - hit.at < PrefetchTtlMs) return hit.response;
    return fetch(url, { credentials: "same-origin", headers: { accept: "text/html" } });
  };

  const swap = (generation: number, entry: HistoryEntry, location: Location, scrollMode: ScrollMode): void => {
    void run(generation, entry, location, scrollMode);
  };

  const state = initRouterState({ history, branches: [], env: "browser", swap });
  root(() => {
    state.owner = getOwner();
  });

  const wrap = <T>(render: () => T): T => provideContext(RouterStateContext, state, render);

  async function run(generation: number, entry: HistoryEntry, location: Location, scrollMode: ScrollMode): Promise<void> {
    const url = history.base + entry.path;
    const fallback = (): void => window.location.assign(url);
    let response: Response;
    try {
      response = await fetchPage(url);
    } catch {
      fallback();
      return;
    }
    if (generation !== state.generation) return;
    if (!response.ok || !(response.headers.get("content-type") ?? "").includes("text/html")) {
      fallback();
      return;
    }
    const incoming = document.implementation.createHTMLDocument("");
    incoming.open();
    const decoder = new TextDecoder();
    const reader = response.body?.getReader();
    let seen = "";
    let isApplied = false;
    let skipped: ReadonlySet<string> | undefined;
    const applyNow = async (): Promise<boolean> => {
      const build = incoming.querySelector('meta[name="reze-build"]')?.getAttribute("content");
      const incomingRoot = incoming.getElementById(options.rootId);
      const liveRoot = document.getElementById(options.rootId);
      if (build !== currentBuild || incomingRoot === null || liveRoot === null) {
        fallback();
        return false;
      }
      const liveLevels = layoutRanges(document);
      const incomingLevels = layoutRanges(incoming);
      let replaceAt = -1;
      if (liveLevels.length > 0 && incomingLevels.length > 0) {
        let shared = 0;
        while (shared < liveLevels.length && shared < incomingLevels.length && liveLevels[shared]!.key === incomingLevels[shared]!.key) shared++;
        const isSamePage = shared === liveLevels.length && shared === incomingLevels.length;
        if (!isSamePage) replaceAt = shared < liveLevels.length ? shared : liveLevels.length - 1;
      }
      const replaced = replaceAt >= 0 && replaceAt < incomingLevels.length ? incomingLevels[replaceAt] : undefined;
      const kept = replaceAt < 0 ? incomingLevels : incomingLevels.slice(0, replaceAt);
      const skip = new Set<string>();
      if (kept.length > 0) {
        for (const comment of tokenComments(incoming)) {
          if (comment.data.startsWith("/rz:")) continue;
          if (!kept.some((range) => isBetween(comment, range.start, range.end))) continue;
          if (replaced !== undefined && isBetween(comment, replaced.start, replaced.end)) continue;
          skip.add(tokenOf(comment));
        }
      }
      if (replaced !== undefined) {
        for (const element of persistRegions(elementsIn(incoming, replaced.start, replaced.end))) {
          for (const comment of commentsIn(element)) {
            if (comment.data.startsWith("rz:") && !comment.data.startsWith("/rz:")) skip.add(tokenOf(comment));
          }
        }
      }
      await syncHead(incoming);
      if (generation !== state.generation) return false;
      const persisted = new Map<string, Element>();
      const isFull = replaced === undefined || replaceAt === 0;
      const target = isFull || replaceAt < 0 ? undefined : liveLevels[replaceAt];
      if (replaceAt === -1) {
        replacePortals(incoming, false);
      } else if (isFull) {
        detachPersisted([...liveRoot.querySelectorAll("[persist]")], persisted);
        disposeIslandsIn(liveRoot);
        liveRoot.replaceChildren(...[...incomingRoot.childNodes].map((node) => document.adoptNode(node)));
        replacePortals(incoming, true);
      } else if (target !== undefined) {
        detachPersisted(elementsIn(document, target.start, target.end), persisted);
        disposeIslandsBetween(target.start, target.end);
        for (let node = target.start.nextSibling; node !== null && node !== target.end; node = target.start.nextSibling) {
          node.parentNode!.removeChild(node);
        }
        for (let node = replaced!.start.nextSibling; node !== null && node !== replaced!.end; node = replaced!.start.nextSibling) {
          target.end.parentNode!.insertBefore(document.adoptNode(node), target.end);
        }
        target.start.data = replaced!.start.data;
        target.end.data = replaced!.end.data;
        replacePortals(incoming, false);
      }
      if (replaceAt !== -1) {
        restorePersisted(isFull || target === undefined ? [...liveRoot.querySelectorAll("[persist]")] : elementsIn(document, target.start, target.end), persisted);
      }
      syncRouteScript(incoming);
      const final = new URL(response.url);
      const finalPath = history.resolve(final);
      const landed = finalPath === undefined ? entry : { ...entry, path: finalPath };
      if (finalPath !== undefined && finalPath !== entry.path) history.replace(finalPath, entry.state);
      const route = readRoute(incoming);
      const routed = routedEntry(route, landed);
      skipped = skip;
      commit(state, routed, parseLocation(routed), stubMatches(route, routed.path), scrollMode);
      return true;
    };
    const apply = async (): Promise<boolean> => {
      const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
      if (typeof document.startViewTransition !== "function" || reduced) return applyNow();
      let applied = false;
      const transition = document.startViewTransition(async () => {
        applied = await applyNow();
      });
      await transition.updateCallbackDone;
      return applied;
    };
    let stream: ReturnType<typeof attachStream> | undefined;
    const chunks = async function* (): AsyncGenerator<string> {
      if (reader === undefined) {
        yield await response.text();
        return;
      }
      for (;;) {
        const { value, done } = await reader.read();
        if (done) return;
        yield decoder.decode(value, { stream: true });
      }
    };
    for await (const text of chunks()) {
      incoming.write(text);
      if (isApplied) continue;
      seen = (seen + text).slice(-(text.length + ShellMarker.length));
      if (!seen.includes(ShellMarker)) continue;
      isApplied = true;
      if (!(await apply())) return;
      stream = attachStream(incoming, document, { wrap, islands: options.islands, skipToken: (token) => skipped?.has(token) === true });
    }
    incoming.close();
    if (!isApplied) {
      if (!(await apply())) return;
      stream = attachStream(incoming, document, { wrap, islands: options.islands, skipToken: (token) => skipped?.has(token) === true });
    }
    stream!.flush(true);
  }

  root(() => {
    const route = readRoute(document);
    const initial = routedEntry(route, history.get());
    state.entry = initial;
    state.target = initial;
    state.targetLocation = parseLocation(initial);
    state.setLocation(state.targetLocation);
    state.setMatches(stubMatches(route, initial.path));
    state.setIsRouting(false);
  });
  loadPositions(state);

  const unlisten = history.listen((entry) => onPop(state, entry));
  const previousRestoration = window.history.scrollRestoration;
  window.history.scrollRestoration = "manual";
  const onPageHide = (): void => persistPositions(state);
  window.addEventListener("pagehide", onPageHide);

  const onClick = (event: MouseEvent): void => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const anchor = anchorOf(event);
    if (anchor === undefined) return;
    const path = anchorPath(state, anchor);
    if (path === undefined) return;
    event.preventDefault();
    navigate(state, path, {
      replace: anchor.hasAttribute("replace"),
      scroll: !anchor.hasAttribute("noscroll"),
      state: anchor.getAttribute("state") ?? undefined,
    });
  };
  const prefetch = (anchor: Element): void => {
    const path = anchorPath(state, anchor);
    if (path === undefined) return;
    const url = history.base + path;
    const hit = prefetched.get(url);
    if (hit !== undefined && Date.now() - hit.at < PrefetchTtlMs) return;
    prefetched.set(url, { at: Date.now(), response: fetch(url, { credentials: "same-origin", headers: { accept: "text/html" } }) });
  };
  let timer: number | undefined;
  const onIntent = (event: Event): void => {
    const anchor = anchorOf(event);
    if (anchor !== undefined) prefetch(anchor);
  };
  const onOver = (event: MouseEvent): void => {
    window.clearTimeout(timer);
    const anchor = anchorOf(event);
    if (anchor !== undefined) timer = window.setTimeout(prefetch, HoverDelayMs, anchor);
  };
  const onOut = (): void => window.clearTimeout(timer);
  document.addEventListener("click", onClick);
  document.addEventListener("mouseover", onOver);
  document.addEventListener("mouseout", onOut);
  document.addEventListener("focusin", onIntent);
  document.addEventListener("touchstart", onIntent, { passive: true });

  return {
    state,
    wrap,
    dispose() {
      unlisten();
      window.clearTimeout(timer);
      window.history.scrollRestoration = previousRestoration;
      window.removeEventListener("pagehide", onPageHide);
      document.removeEventListener("click", onClick);
      document.removeEventListener("mouseover", onOver);
      document.removeEventListener("mouseout", onOut);
      document.removeEventListener("focusin", onIntent);
      document.removeEventListener("touchstart", onIntent);
    },
  };
}
