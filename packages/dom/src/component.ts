import { root, untrack } from "@rezejs/signals";
import { debugHook } from "@rezejs/signals/devtools";

import { insert } from "./insert";
import type { JSX } from "./jsx";

/** Calls `Comp` once, untracked, so its reads never re-run the caller. */
export function createComponent<P>(Comp: (props: P) => JSX.Element, props: P): JSX.Element {
  if (process.env.NODE_ENV !== "production" && debugHook !== undefined) {
    return debugHook.component(Comp.name, () => untrack(() => Comp(props)));
  }
  return untrack(() => Comp(props));
}

/** Mounts `code()` after the existing children of `element`; the returned function disposes it and empties `element`. */
export function render(code: () => JSX.Element, element: Element): () => void {
  const dispose = root((dispose) => {
    insert(element, code(), element.firstChild === null ? undefined : null);
    return dispose;
  });
  return () => {
    dispose();
    element.textContent = "";
  };
}
