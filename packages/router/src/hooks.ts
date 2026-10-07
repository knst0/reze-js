import { onCleanup, useContext } from "@rezejs/signals";
import { computed } from "reze-js";

import { matchPath, type PathMatch } from "./match";
import { addLeaveListener, navigate, RouterContext, type RouterState } from "./navigation";
import type {
  BeforeLeaveEvent,
  Location,
  Navigate,
  NavigateOptions,
  Params,
  ParamsFor,
  RouteMatch,
  RoutePattern,
  SearchInit,
  SearchValue,
} from "./types";

const NoParams: Params = Object.freeze({});

/** `matchPath` narrowed to pattern `P` when the plugin registered it. */
export type PathMatchFor<P extends string> = P extends RoutePattern ? { params: ParamsFor<P>; path: string } : PathMatch;

export function useRouter(): RouterState {
  const state = useContext(RouterContext);
  if (state === undefined) throw new Error("[reze-router] hooks must be called inside <Router>");
  return state;
}

export function useLocation(): () => Location {
  return useRouter().location;
}

/** The deepest match's params, merged across its layouts; `from` narrows them to a registered pattern. */
export function useParams(): () => Params;
export function useParams<P extends RoutePattern>(from: P): () => ParamsFor<P>;
export function useParams(_from?: string): () => Params {
  const state = useRouter();
  return () => {
    const matches = state.matches();
    return matches[matches.length - 1]?.params ?? NoParams;
  };
}

export function useNavigate(): Navigate {
  const state = useRouter();
  return (to, options) => navigate(state, to, options);
}

/**
 * The query and a setter that merges into it: `null`/`undefined` delete a key, an array sets one entry per non-nullish item.
 * The setter builds on the latest navigation, even one still loading, and defaults to `scroll: false`.
 */
export function useSearchParams(): [() => Location["query"], (next: SearchInit, options?: NavigateOptions) => void] {
  const state = useRouter();
  const query = (): Location["query"] => state.location().query;
  const setQuery = (next: SearchInit, options?: NavigateOptions): void => {
    const current = state.targetLocation!;
    const search = new URLSearchParams(current.search);
    for (const key in next) {
      const value = next[key];
      if (value === null || value === undefined) {
        search.delete(key);
      } else if (!Array.isArray(value)) {
        search.set(key, String(value));
      } else {
        search.delete(key);
        for (const item of value as readonly SearchValue[]) {
          if (item !== null && item !== undefined) search.append(key, String(item));
        }
      }
    }
    const qs = search.toString();
    navigate(state, current.pathname + (qs ? "?" + qs : "") + current.hash, { scroll: false, ...options });
  };
  return [query, setQuery];
}

/** Matches `pattern` against the current pathname; a trailing `/*` matches any deeper path too. */
export function useMatch<P extends string>(pattern: () => P): () => PathMatchFor<P> | undefined {
  const state = useRouter();
  const match = computed(matchPath(pattern(), state.location().pathname) as PathMatchFor<P> | undefined);
  return () => match;
}

/** `true` while a navigation waits for route modules to load. */
export function useIsRouting(): () => boolean {
  return useRouter().isRouting;
}

export function useCurrentMatches(): () => readonly RouteMatch[] {
  const state = useRouter();
  const matches = computed(state.matches().map(({ path, params, data, info }) => ({ path, params, data, info })));
  return () => matches;
}

/** Calls `listener` before each navigation away from the current path and before the document unloads, until the calling owner is disposed. */
export function useBeforeLeave(listener: (event: BeforeLeaveEvent) => void): void {
  onCleanup(addLeaveListener(useRouter(), listener));
}
