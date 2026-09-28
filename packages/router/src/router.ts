import { createComponent, type JSX } from "@rezejs/dom";
import { computed, getOwner, onCleanup, provideContext, signal, untrack } from "@rezejs/signals";

import { createBrowserHistory, type RouterHistory } from "./history";
import { installLinks } from "./links";
import { compileRoutes } from "./match";
import { onPop, parseLocation, RouterContext, start, type ActiveMatch, type RouterState } from "./navigation";
import type { RouteDefinition } from "./types";

/** Read once, when the router is created. */
export interface RouterProps {
  routes: readonly RouteDefinition[];
  /** Default `createBrowserHistory()`. */
  history?: RouterHistory;
  /** Wraps every page; `children` renders the matched route. */
  root?: (props: { children: JSX.Element }) => JSX.Element;
  /** Default `true`: load and preload a link's route on hover, focus or touch. */
  preload?: boolean;
}

const NoMatches: readonly ActiveMatch[] = [];

function renderLevel(state: RouterState, depth: number): JSX.Element {
  const match = state.matches()[depth]!;
  if (match.error !== undefined) throw match.error;
  const Comp = match.route.component;
  if (Comp === undefined) return outlet(state, depth + 1);
  let children: JSX.Element;
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
    location,
    setLocation,
    matches,
    setMatches,
    isRouting,
    setIsRouting,
    pendingKey,
    setPendingKey,
    links: undefined,
    generation: 0,
    leaveListeners: new Set(),
    ignorePop: false,
    skipNextGuard: false,
    positions: undefined,
  };
  const root = props.root;
  const isPreloading = props.preload !== false;
  return provideContext(RouterContext, state, () => {
    state.owner = getOwner();
    start(state, history.get(), "initial", "none");
    const unlisten = history.listen((entry) => onPop(state, entry));
    const scrollRestoration = history.scroll ? window.history.scrollRestoration : undefined;
    if (scrollRestoration !== undefined) window.history.scrollRestoration = "manual";
    const uninstallLinks = installLinks(state, isPreloading);
    onCleanup(() => {
      state.generation++;
      unlisten();
      uninstallLinks();
      if (scrollRestoration !== undefined) window.history.scrollRestoration = scrollRestoration;
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
