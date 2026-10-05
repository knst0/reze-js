import { createComponent, type JSX } from "@rezejs/dom";
import { computed, getOwner, onCleanup, provideContext, untrack } from "@rezejs/signals";

import { createBrowserHistory, type HistoryEntry, type RouterHistory } from "./history";
import { installLinks } from "./links";
import { compileRoutes, matchBranches } from "./match";
import {
  initRouterState,
  loadPositions,
  onPop,
  parseLocation,
  persistPositions,
  RouterContext,
  start,
  type RouterState,
} from "./navigation";
import type { OutputMatch, PageMetadata, PathsTree, RouteDefinition } from "./types";

export interface RouterConfig {
  routes: readonly RouteDefinition[];
  /** Prebuilt href builders (`paths` from `virtual:reze-routes`, or `buildPaths` for hand-written tables); the factory never builds them, so apps that skip it skip the code. */
  paths?: PathsTree;
  /** Default `createBrowserHistory()`. */
  history?: RouterHistory;
  /** Route same-origin `<a>` clicks anywhere in the document; `false` leaves them to the browser, so the router moves only through `useNavigate`. Default `true` for window histories (browser, hash), `false` for a memory history, which does not own the page's links. */
  links?: boolean;
  /** Default `true`: load and preload a link's route on hover, focus or touch. Needs `links`. */
  preload?: boolean;
  /**
   * Template head metadata the merged page metadata restores absent fields to. The hydration boot passes the payload
   * `headDefaults`; without it the router captures the live document on first apply.
   */
  headDefaults?: PageMetadata;
}

export interface RouterInstance {
  (props: { root?: (props: { children: JSX.Element }) => JSX.Element }): JSX.Element;
  /** Matches `url` root-to-leaf without rendering; `[]` when nothing matches. */
  readonly match: (url: string) => OutputMatch[];
  /** The configured `paths`, or `undefined` when the factory got none. */
  readonly paths: PathsTree | undefined;
  readonly routes: readonly RouteDefinition[];
}

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
/**
 * Binds an already-settled SSG state to the router outlet without starting a navigation, installing listeners, or
 * touching history. Preparation owns matching, data and redirect capture; mount the result inside the page scope.
 */
export function createSettledRouter(
  state: RouterState,
): (props: { root?: (props: { children: JSX.Element }) => JSX.Element }) => JSX.Element {
  return function SettledRouter(props: { root?: (props: { children: JSX.Element }) => JSX.Element }): JSX.Element {
    const root = props.root;
    return provideContext(RouterContext, state, () => {
      state.owner = getOwner();
      const host = state.commitHost;
      if (host !== undefined) {
        onCleanup(() => {
          state.generation++;
        });
        host.deferCommit(() => {
          if (state.history.scroll) loadPositions(state);
          installRouterListeners(state, true, true, state.history.scroll);
        });
      }
      if (root === undefined) return outlet(state, 0);
      let children: JSX.Element;
      return createComponent(root, {
        get children() {
          return (children ??= outlet(state, 0));
        },
      });
    });
  };
}

/** Compiles `routes` once and returns the router component; mount it with an optional `root` shell. */
export function createRouter(config: RouterConfig & { paths: PathsTree }): RouterInstance & { readonly paths: PathsTree };
export function createRouter(config: RouterConfig): RouterInstance;
export function createRouter(config: RouterConfig): RouterInstance {
  const history = config.history ?? createBrowserHistory();
  const branches = compileRoutes(config.routes);
  const isLinking = config.links ?? history.scroll;
  const isPreloading = config.preload !== false;

  const match = (url: string): OutputMatch[] => {
    const entry: HistoryEntry = { path: url, state: undefined, index: -1 };
    const hit = matchBranches(branches, parseLocation(entry).pathname);
    if (hit === undefined) return [];
    return hit.branch.routes.map((route) => ({ path: hit.path, pattern: route.pattern, params: hit.params, info: route.info }));
  };

  function Router(props: { root?: (props: { children: JSX.Element }) => JSX.Element }): JSX.Element {
    const state = initRouterState({
      history,
      branches,
      env: "browser",
      headBaseline: config.headDefaults,
    });
    const root = props.root;
    return provideContext(RouterContext, state, () => {
      state.owner = getOwner();
      const isScrollManaged = state.env === "browser" && history.scroll;
      if (isScrollManaged) loadPositions(state);
      start(state, history.get(), "initial", "initial");
      installRouterListeners(state, isLinking, isPreloading, isScrollManaged);
      if (root === undefined) return outlet(state, 0);
      let children: JSX.Element;
      return createComponent(root, {
        get children() {
          return (children ??= outlet(state, 0));
        },
      });
    });
  }

  return Object.assign(Router, { match, paths: config.paths, routes: config.routes });
}

function installRouterListeners(state: RouterState, isLinking: boolean, isPreloading: boolean, isScrollManaged: boolean): void {
  const unlisten = state.history.listen((entry) => onPop(state, entry));
  const scrollRestoration = isScrollManaged && typeof window !== "undefined" ? window.history.scrollRestoration : undefined;
  const onPageHide = (): void => persistPositions(state);
  if (scrollRestoration !== undefined) {
    window.history.scrollRestoration = "manual";
    addEventListener("pagehide", onPageHide);
  }
  if (isLinking) onCleanup(installLinks(state, isPreloading));
  onCleanup(() => {
    state.generation++;
    unlisten();
    removeEventListener("beforeunload", state.onUnload);
    if (scrollRestoration === undefined) return;
    removeEventListener("pagehide", onPageHide);
    window.history.scrollRestoration = scrollRestoration;
  });
}
