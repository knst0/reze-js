/**
 * Hydration keys: the server stamps each template root with `data-hk`, the client claims the
 * element with the same key. A key is the path of component calls down to the template plus
 * the template's index among the templates created directly in that component, so it only
 * depends on the order in which one component creates its templates.
 */
interface KeyScope {
  id: string;
  count: number;
}

let scope: KeyScope | undefined;

/** The key of the next template, while rendering or hydrating; otherwise `undefined`. */
export function nextHydrationKey(): string | undefined {
  return scope && scope.id + scope.count++;
}

/** Runs `fn` with template keys starting at `id + 0`: `""` for `renderToString` and `hydrate`. */
export function withKeyScope<T>(id: string, fn: () => T): T {
  const parent = scope;
  scope = { id, count: 0 };
  try {
    return fn();
  } finally {
    scope = parent;
  }
}

/** The id of the next component key scope under the current one, while rendering or hydrating. */
export function nextComponentScope(): string | undefined {
  return scope && scope.id + scope.count++ + "-";
}

/** Runs a component body under its own key prefix. */
export function withComponentKeys<T>(fn: () => T): T {
  const id = nextComponentScope();
  return id === undefined ? fn() : withKeyScope(id, fn);
}
