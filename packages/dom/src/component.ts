import { root, untrack } from "@rezejs/signals";
import { debugHook } from "@rezejs/signals/devtools";

import { insert } from "./insert";
import type { JSX } from "./jsx";

/** Calls `Comp` once, untracked, so its reads never re-run the caller. */
export function createComponent<P>(Comp: (props: P) => JSX.Element, props: P): JSX.Element {
  if (process.env.NODE_ENV !== "production" && debugHook !== undefined) {
    return debugHook.component(Comp.name, () => untrack(() => Comp(props)));
  }
  return untrack(Comp, props);
}

/** Mounts `code()` into `element`, replacing a prerendered shell or any previous content; the returned function disposes it and empties `element`. */
export function render(code: () => JSX.Element, element: Element): () => void {
  const dispose = root((dispose) => {
    element.textContent = "";
    insert(element, code(), undefined);
    return dispose;
  });
  return () => {
    dispose();
    element.textContent = "";
  };
}
