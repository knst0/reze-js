import { matchBranches } from "./match";
import { loadBranch, navigate, parseLocation, resolveHref, type RouterState } from "./navigation";

const SvgNamespace = "http://www.w3.org/2000/svg";
const HoverDelayMs = 20;

function anchorOf(event: Event): Element | undefined {
  for (const node of event.composedPath()) {
    if ((node as Node).nodeName?.toUpperCase() === "A") return node as Element;
  }
  return undefined;
}

function anchorPath(state: RouterState, anchor: Element): string | undefined {
  const isSvg = anchor.namespaceURI === SvgNamespace;
  const href = isSvg ? (anchor as SVGAElement).href.baseVal : anchor.getAttribute("href");
  const target = isSvg ? (anchor as SVGAElement).target.baseVal : (anchor as HTMLAnchorElement).target;
  if (!href || (target && target !== "_self") || anchor.hasAttribute("download") || /\bexternal\b/.test(anchor.getAttribute("rel") ?? "")) {
    return undefined;
  }
  return resolveHref(state, href);
}

function ignoreRejection(): void {}

/** Loads `path`'s route modules and runs their `preload` with intent `"preload"`; `false` when a module failed to load. */
async function preloadPath(state: RouterState, path: string): Promise<boolean> {
  const location = parseLocation({ path, state: undefined, index: 0 });
  const match = matchBranches(state.branches, location.pathname);
  if (match === undefined) return true;
  await loadBranch(match);
  for (const route of match.branch.routes) {
    if (!route.isLoaded) return false;
  }
  for (const route of match.branch.routes) {
    if (route.preload === undefined) continue;
    try {
      const data = route.preload({ params: match.params, location, intent: "preload" });
      if (data instanceof Promise) data.catch(ignoreRejection);
    } catch {
      continue;
    }
  }
  return true;
}

/** Routes same-origin anchor clicks through `state` and, with `isPreloading`, preloads hovered links; returns the remover. */
export function installLinks(state: RouterState, isPreloading: boolean): () => void {
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
  document.addEventListener("click", onClick);
  if (!isPreloading) return () => document.removeEventListener("click", onClick);

  let lastPreloaded: string | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const preloadAnchor = (anchor: Element): void => {
    const path = anchorPath(state, anchor);
    if (path === undefined || path === lastPreloaded) return;
    lastPreloaded = path;
    void preloadPath(state, path).then((isLoaded) => {
      if (!isLoaded && lastPreloaded === path) lastPreloaded = undefined;
    });
  };
  const onIntent = (event: Event): void => {
    const anchor = anchorOf(event);
    if (anchor !== undefined) preloadAnchor(anchor);
  };
  const onOver = (event: MouseEvent): void => {
    clearTimeout(timer);
    const anchor = anchorOf(event);
    if (anchor !== undefined) timer = setTimeout(preloadAnchor, HoverDelayMs, anchor);
  };
  const onOut = (): void => clearTimeout(timer);
  document.addEventListener("mouseover", onOver);
  document.addEventListener("mouseout", onOut);
  document.addEventListener("focusin", onIntent);
  document.addEventListener("touchstart", onIntent, { passive: true });
  return () => {
    clearTimeout(timer);
    document.removeEventListener("click", onClick);
    document.removeEventListener("mouseover", onOver);
    document.removeEventListener("mouseout", onOut);
    document.removeEventListener("focusin", onIntent);
    document.removeEventListener("touchstart", onIntent);
  };
}
