import { onCleanup, untrack } from "@rezejs/signals";
import { renderEffect } from "@rezejs/signals/render";

import { insert } from "./insert";
import type { JSX } from "./jsx";

/**
 * Runtime of `<Portal>`: builds `children` once under the current owner, so context and errors still reach it, and keeps
 * the result in `mount()` (the body when it is absent or `null`), moving the same nodes when `mount()` changes. Disposing
 * the owner removes it.
 */
export function portal(children: () => JSX.Element, mount?: () => Node | null | undefined): void {
  const start = document.createTextNode("");
  const end = document.createTextNode("");
  const detached = document.createDocumentFragment();
  detached.append(start, end);
  insert(detached, untrack(children), end);
  renderEffect(() => {
    moveRange(start, end, (mount !== undefined && mount()) || document.body);
    onCleanup(() => moveRange(start, end, detached));
  });
}

/** Appends the nodes from `start` through `end` to `parent`; nodes removed from between them by others stay removed. */
function moveRange(start: Node, end: Node, parent: Node): void {
  for (let node: Node | null = start, next: Node | null; node !== null && node !== end; node = next) {
    next = node.nextSibling;
    parent.appendChild(node);
  }
  parent.appendChild(end);
}
