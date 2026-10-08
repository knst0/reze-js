import { computed, untrack, type AsyncContext } from "@rezejs/signals";
import { internalAsyncComputed } from "@rezejs/signals/internal/resource";

import { createComponent } from "../component";
import { errored } from "../errored";
import { branch, choose } from "../flow";
import type { JSX } from "../jsx";
import { list } from "../list";
import { asyncComponent, loading } from "../loading";
import { repeat } from "../repeat";
import type { RangeKind, Site } from "./protocol";
import { managedRange } from "./range";
import { preparingSession } from "./session";

function range(kind: RangeKind, role: string, site: Site, build: () => unknown): JSX.Element {
  const session = preparingSession();
  return (session === undefined ? untrack(build) : managedRange(session, kind, role, site, build)) as JSX.Element;
}

export function prepareComponent<P>(site: Site, component: (props: P) => JSX.Element, props: P): JSX.Element {
  if (preparingSession() === undefined) return createComponent(component, props);
  return range("component", "c", site, () => untrack(component, props));
}

export function prepareFragment(site: Site, build: () => unknown): JSX.Element {
  return range("fragment", "f", site, build);
}

export function prepareShow<T>(
  site: Site,
  when: () => T,
  child: (value: () => T) => JSX.Element,
  fallback?: () => JSX.Element,
): JSX.Element {
  if (preparingSession() === undefined) return branch(when, child, fallback);
  return range("branch", "s", site, () =>
    branch(
      when,
      (value) => range("branch", "b1", site, () => child(value)),
      fallback === undefined ? undefined : () => range("branch", "b0", site, fallback),
    ),
  );
}

export function prepareChoose(
  site: Site,
  whens: readonly (() => unknown)[],
  children: readonly ((value: () => unknown) => JSX.Element)[],
  fallback?: () => JSX.Element,
): JSX.Element {
  if (preparingSession() === undefined) return choose(whens, children, fallback);
  return range("branch", "w", site, () =>
    choose(
      whens,
      children.map((child, index) => (value) => range("branch", `b${index.toString(36)}`, site, () => child(value))),
      fallback === undefined ? undefined : () => range("branch", "fallback", site, fallback),
    ),
  );
}

export function prepareList<T>(
  site: Site,
  each: () => readonly T[] | null | undefined | false,
  map: (item: never, index: never) => JSX.Element,
  fallback?: () => JSX.Element,
  keyed?: boolean | ((item: T) => unknown),
): JSX.Element {
  type MapRow = (item: never, index: never) => JSX.Element;
  type MakeList = (
    each: () => readonly T[] | null | undefined | false,
    map: MapRow,
    fallback: (() => JSX.Element) | undefined,
    keyed: boolean | ((item: T) => unknown) | undefined,
  ) => () => JSX.Element;
  const makeList = list as MakeList;
  if (preparingSession() === undefined) return makeList(each, map, fallback, keyed);
  return range("list", "l", site, () => {
    const row: MapRow = (item, index) => range("row", "row", site, () => map(item, index));
    Object.defineProperty(row, "length", { value: map.length });
    return makeList(each, row, fallback === undefined ? undefined : () => range("branch", "fallback", site, fallback), keyed);
  });
}

export function prepareRepeat(
  site: Site,
  count: () => number,
  map: (index: number) => JSX.Element,
  fallback?: () => JSX.Element,
): JSX.Element {
  if (preparingSession() === undefined) return repeat(count, map, fallback);
  return range("list", "repeat", site, () =>
    repeat(
      count,
      (index) => range("row", "row", site, () => map(index)),
      fallback === undefined ? undefined : () => range("branch", "fallback", site, fallback),
    ),
  );
}

export function prepareRows(site: Site, count: number, map: (index: number) => unknown): JSX.Element {
  return range("list", "rows", site, () => {
    const rows: JSX.Element[] = [];
    for (let index = 0; index < count; index++) rows.push(range("row", "row", site, () => map(index)));
    return rows;
  });
}

export function prepareLoading(site: Site, child: () => JSX.Element, fallback?: () => JSX.Element): JSX.Element {
  if (preparingSession() === undefined) return loading(child, fallback);
  return range("branch", "loading", site, () =>
    loading(
      () => range("branch", "content", site, child),
      fallback === undefined ? undefined : () => range("branch", "fallback", site, fallback),
    ),
  );
}

export function prepareErrored(
  site: Site,
  child: () => JSX.Element,
  fallback?: (error: unknown, reset: () => void) => JSX.Element,
): JSX.Element {
  if (preparingSession() === undefined) return errored(child, fallback);
  return range("branch", "errored", site, () =>
    errored(
      () => range("branch", "content", site, child),
      fallback === undefined ? undefined : (error, reset) => range("branch", "fallback", site, () => fallback(error, reset)),
    ),
  );
}

export function prepareAsyncComponent<V extends unknown[], R>(
  site: Site,
  load: (context: AsyncContext) => PromiseLike<V>,
  body: (values: () => V, isPending: () => boolean) => R,
): JSX.Element {
  if (preparingSession() === undefined) return asyncComponent(load, body) as JSX.Element;
  return range("async", "async", site, () => {
    const step = internalAsyncComputed(load);
    const values = (): V => step.value()!;
    const isLoaded = computed(() => step.value() !== undefined);
    const render = (): R => body(values, () => false);
    const view = computed(() => (isLoaded() ? untrack(render) : undefined));
    return computed(() => {
      const current = view();
      const error = step.error();
      if (error !== undefined) throw error;
      return current;
    });
  });
}

export function prepareAsyncViews(
  site: Site,
  children: () => JSX.Element,
  failure?: (error: unknown, reset: () => void) => JSX.Element,
): JSX.Element {
  return failure === undefined ? children() : prepareErrored(site, children, failure);
}

export function prepareDynamic<P>(
  site: Site,
  source: () => ((props: P) => JSX.Element) | null | undefined | false,
): (props: P) => JSX.Element {
  return (props) => {
    const type = computed(source);
    return () => {
      const component = type();
      return component ? prepareComponent(site, component, props) : undefined;
    };
  };
}
