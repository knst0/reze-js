import { renderRoot, runComponent } from "@rezejs/signals/internal/scope";
import { profileComponent } from "@rezejs/signals/profile";

import { insert } from "./insert";
import type { JSX } from "./jsx";

/** Calls `Comp` once, untracked, so its reads never re-run the caller. */
export function createComponent<P>(Comp: (props: P) => JSX.Element, props: P, profile?: string): JSX.Element {
  if (process.env.NODE_ENV !== "production") {
    const count = typeof props === "object" && props !== null ? Object.keys(props).length : 0;
    return profileComponent(Comp.name, profile, count, () => runComponent(Comp, props));
  }
  return runComponent(Comp, props);
}

/** Replaces all existing content in `element` with `code()`; the returned function disposes it and empties `element`. */
export function render(code: () => JSX.Element, element: Element): () => void {
  const dispose = renderRoot((dispose) => {
    element.textContent = "";
    insert(element, code(), undefined);
    return dispose;
  });
  return () => {
    dispose();
    element.textContent = "";
  };
}
