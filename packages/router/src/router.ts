import { createComponent, type JSX } from "@rezejs/dom";
import { computed, getOwner, onCleanup, provideContext, signal, untrack } from "@rezejs/signals";

import { createBrowserHistory, type RouterHistory } from "./history";
import { installLinks } from "./links";
import { compileRoutes } from "./match";
import {
  isLeavePrevented,
  loadPositions,
  onPop,
  parseLocation,
  persistPositions,
  RouterContext,
  start,
  type ActiveMatch,
  type RouterState,
} from "./navigation";
import type { RouteDefinition } from "./types";

/** Read once, when the router is created. */
export interface RouterProps {
  routes: readonly RouteDefinition[];
  /** Default `createBrowserHistory()`. */
  history?: RouterHistory;
  /** Wraps every page; `children` renders the matched route. */
  root?: (props: { children: JSX.Element }) => JSX.Element;
  /** Route same-origin `<a>` clicks anywhere in the document; `false` leaves them to the browser, so the router moves only through `useNavigate`. Default `true` for window histories (browser, hash), `false` for a memory history, which does not own the page's links. */
  links?: boolean;
  /** Default `true`: load and preload a link's route on hover, focus or touch. Needs `links`. */
  preload?: boolean;
}

function ignoreRetry(): void {}

const NoMatches: readonly ActiveMatch[] = [];

function renderLevel(state: RouterState, depth: number): JSX.Element {
  const match = state.matches()[depth]!;
  if (match.error !== undefined) throw match.error;
  const Comp = match.route.component;
  if (Comp === undefined) return outlet(state, depth + 1);
  let children: JSX.Element;
  state.renderDepth++;
  try {
    return createComponent(Comp, {
      get params() {
        return state.matches()[depth]?.params;
      },
      get data() {
        return state.matches()[depth]?.data;
      },
      get location() {
        return state.location();
      },
      get children() {
        return (children ??= outlet(state, depth + 1));
      },
    });
  } finally {
    state.renderDepth--;
  }
}

function outlet(state: RouterState, depth: number): () => JSX.Element {
  const key = computed((): unknown => {
    const match = state.matches()[depth];
    return match?.error === undefined ? match?.route : match;
  });
  return computed(() => (key() === undefined ? undefined : untrack(() => renderLevel(state, depth))));
}

export function Router(props: RouterProps): JSX.Element {
  const history = props.history ?? createBrowserHistory();
  const [location, setLocation] = signal(parseLocation(history.get()));
  const [matches, setMatches] = signal(NoMatches);
  const [isRouting, setIsRouting] = signal(true);
  const [pendingKey, setPendingKey] = signal<string | undefined>(undefined);
  const state: RouterState = {
    history,
    branches: compileRoutes(props.routes),
    owner: undefined,
    entry: undefined,
    target: undefined,
    targetLocation: undefined,
    location,
    setLocation,
    matches,
    setMatches,
    isRouting,
    setIsRouting,
    pendingKey,
    setPendingKey,
    links: undefined,
    renderDepth: 0,
    matchedPathname: undefined,
    lastMatch: undefined,
    generation: 0,
    leaveListeners: new Set(),
    onUnload: (event) => {
      if (!isLeavePrevented(state, null, {}, ignoreRetry)) return;
      event.preventDefault();
      event.returnValue = true;
    },
    ignorePop: false,
    skipNextGuard: false,
    positions: undefined,
  };
  const root = props.root;
  const isLinking = props.links ?? history.scroll;
  const isPreloading = props.preload !== false;
  return provideContext(RouterContext, state, () => {
    state.owner = getOwner();
    const isScrollManaged = history.scroll;
    if (isScrollManaged) loadPositions(state);
    start(state, history.get(), "initial", "initial");
    const unlisten = history.listen((entry) => onPop(state, entry));
    const scrollRestoration = isScrollManaged ? window.history.scrollRestoration : undefined;
    const onPageHide = (): void => persistPositions(state);
    if (scrollRestoration !== undefined) {
      window.history.scrollRestoration = "manual";
      addEventListener("pagehide", onPageHide);
    }
    const uninstallLinks = isLinking ? installLinks(state, isPreloading) : undefined;
    onCleanup(() => {
      state.generation++;
      unlisten();
      uninstallLinks?.();
      removeEventListener("beforeunload", state.onUnload);
      if (scrollRestoration === undefined) return;
      removeEventListener("pagehide", onPageHide);
      window.history.scrollRestoration = scrollRestoration;
    });
    if (root === undefined) return outlet(state, 0);
    let children: JSX.Element;
    return createComponent(root, {
      get children() {
        return (children ??= outlet(state, 0));
      },
    });
  });
}
