import { attachStream, disposeIslandsIn, type BootOptions } from "@rezejs/dom/internal/client";
import { getOwner, provideContext, root } from "@rezejs/signals";

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
  RouterContext,
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
const HeadSingletons = ['meta[name="description"]', 'link[rel="canonical"]', 'meta[name="robots"]'];

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
        meta: {},
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

function awaitLoaded(link: HTMLLinkElement): Promise<void> {
  return new Promise((resolve) => {
    link.addEventListener("load", () => resolve(), { once: true });
    link.addEventListener("error", () => resolve(), { once: true });
  });
}

async function syncHead(incoming: Document): Promise<void> {
  if (incoming.title !== "") document.title = incoming.title;
  for (const selector of HeadSingletons) {
    document.head.querySelector(selector)?.remove();
    const next = incoming.head.querySelector(selector);
    if (next !== null) document.head.append(document.adoptNode(next));
  }
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

  const wrap = <T>(render: () => T): T => provideContext(RouterContext, state, render);

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
    const apply = async (): Promise<boolean> => {
      const build = incoming.querySelector('meta[name="reze-build"]')?.getAttribute("content");
      const incomingRoot = incoming.getElementById(options.rootId);
      const liveRoot = document.getElementById(options.rootId);
      if (build !== currentBuild || incomingRoot === null || liveRoot === null) {
        fallback();
        return false;
      }
      await syncHead(incoming);
      if (generation !== state.generation) return false;
      disposeIslandsIn(liveRoot);
      liveRoot.replaceChildren(...[...incomingRoot.childNodes].map((node) => document.adoptNode(node)));
      replaceBetween(document, incoming, "rz:p");
      const final = new URL(response.url);
      const finalPath = history.resolve(final);
      const landed = finalPath === undefined ? entry : { ...entry, path: finalPath };
      if (finalPath !== undefined && finalPath !== entry.path) history.replace(finalPath, entry.state);
      const route = readRoute(incoming);
      const routed = routedEntry(route, landed);
      commit(state, routed, parseLocation(routed), stubMatches(route, routed.path), undefined, scrollMode);
      return true;
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
      stream = attachStream(incoming, document, { wrap, islands: options.islands });
    }
    incoming.close();
    if (!isApplied) {
      if (!(await apply())) return;
      stream = attachStream(incoming, document, { wrap, islands: options.islands });
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
