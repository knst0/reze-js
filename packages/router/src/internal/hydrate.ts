import { setAttribute, type JSX } from "@rezejs/dom";
import { sessionFor } from "@rezejs/dom/internal/hydrate";

import { createBrowserHistory, routerBase, type HistoryEntry, type RouterHistory } from "../history";
import { compileRoutes } from "../match";
import { commit, initRouterState, loadBranch, matchPathname, parseLocation, type ActiveMatch, type RouterCommitHost } from "../navigation";
import { createSettledRouter } from "../router";
import { installLinkTarget } from "../target";
import type { PageMetadata, Params, RouteDefinition } from "../types";

installLinkTarget({
  setAttribute(node, name, value) {
    const element = node as Element;
    const session = sessionFor(element);
    if (session === undefined) setAttribute(element, name, value);
    else session.staging.attribute(element, name, value);
  },
  getAttribute: (node, name) => (node as Element).getAttribute(name),
});

export interface RouterHydrationSeed {
  readonly params: unknown;
  readonly hasData: boolean;
  readonly data?: unknown;
}

export interface RouterHydrationHost extends RouterCommitHost {
  run<T>(fn: () => T): T;
  /** Consumes the recorded route input root-to-leaf, throwing on missing mandatory slots. */
  readRoute(id: string): RouterHydrationSeed;
  readonly headDefaults: PageMetadata;
}

export type HydratedRouter = (props: { root?: (props: { children: JSX.Element }) => JSX.Element }) => JSX.Element;

function deferredHistory(base: string, host: RouterHydrationHost): RouterHistory {
  const prefix = routerBase(base);
  const strip = (pathname: string): string | undefined => pathname === prefix ? "/" : pathname.startsWith(prefix + "/") ? pathname.slice(prefix.length) : undefined;
  const stored = window.history.state as { reze?: unknown; index: number; state: unknown } | null;
  const entry: HistoryEntry = {
    path: (strip(location.pathname) ?? location.pathname) + location.search + location.hash,
    state: typeof stored === "object" && stored !== null && stored.reze === 1 ? stored.state : stored,
    index: typeof stored === "object" && stored !== null && stored.reze === 1 ? stored.index : window.history.length - 1,
  };
  let live: RouterHistory | undefined;
  host.deferCommit(() => { live = createBrowserHistory(base); });
  const active = (): RouterHistory => {
    if (live === undefined) throw new Error("[reze-router] history mutation before hydration commit");
    return live;
  };
  return {
    get: () => live?.get() ?? entry,
    push: (path, state) => active().push(path, state),
    replace: (path, state) => active().replace(path, state),
    go: delta => active().go(delta),
    listen: listener => active().listen(listener),
    resolve(url) {
      const pathname = strip(url.pathname);
      return pathname === undefined ? undefined : pathname + url.search + url.hash;
    },
    base: prefix,
    scroll: true,
  };
}

/** Loads the initial branch before view setup; route callbacks are replaced by recorded inputs and browser writes wait for commit. */
export async function prepareHydratedRouter(routes: readonly RouteDefinition[], host: RouterHydrationHost, base = ""): Promise<HydratedRouter> {
  const state = host.run(() => initRouterState({
    history: deferredHistory(base, host),
    branches: compileRoutes(routes),
    env: "hydrate",
    headBaseline: host.headDefaults,
    commitHost: host,
  }));
  const entry = state.history.get();
  const location = parseLocation(entry);
  state.target = entry;
  state.targetLocation = location;
  const match = matchPathname(state, location.pathname);
  if (match !== undefined) await host.run(() => loadBranch(match));
  return host.run(() => {
    const matches: ActiveMatch[] = match === undefined ? [] : match.branch.routes.map(route => {
      const seed = host.readRoute(route.id);
      return {
        route,
        path: match.path,
        params: seed.params as Params,
        data: seed.hasData ? seed.data : undefined,
        hasData: seed.hasData,
        error: route.isLoaded ? undefined : route.loadError,
        meta: {},
        info: route.info,
      };
    });
    commit(state, entry, location, matches, undefined, "none");
    return createSettledRouter(state);
  });
}
