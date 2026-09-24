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

/** Runs `fn` with fresh keys: `renderToString` and `hydrate` start here. */
export function withKeyRoot<T>(fn: () => T): T {
  const parent = scope;
  scope = { id: "", count: 0 };
  try {
    return fn();
  } finally {
    scope = parent;
  }
}

/** Runs a component body under its own key prefix. */
export function withComponentKeys<T>(fn: () => T): T {
  const parent = scope;
  if (parent === undefined) return fn();
  scope = { id: parent.id + parent.count++ + "-", count: 0 };
  try {
    return fn();
  } finally {
    scope = parent;
  }
}
