import { onCleanup } from "@rezejs/signals";
import { renderEffect } from "@rezejs/signals/render";

import type { JSX } from "../jsx";
import { portal } from "../portal";
import { managedPlans } from "./plan";
import { HydrationError, type Site } from "./protocol";
import { managedRange } from "./range";
import { preparingSession } from "./session";

function move(start: Node, end: Node, parent: Node): void {
  if (start.parentNode === parent && end.parentNode === parent) return;
  for (let node: Node | null = start; node !== null;) {
    const next: Node | null = node.nextSibling;
    parent.appendChild(node);
    if (node === end) return;
    node = next;
  }
  throw new HydrationError("portal end boundary was removed");
}

export function claimPortal(site: Site, child: () => JSX.Element, mount?: () => Node | null | undefined): JSX.Element {
  const session = preparingSession();
  if (session === undefined) {
    portal(child, mount);
    return undefined;
  }
  return managedRange(session, "portal", "portal", site, () => {
    const content = managedRange(session, "portal", "content", site, child);
    const plan = managedPlans.get(content)!;
    session.portals.push(plan);
    session.deferCommit(() => {
      const claimed = plan.claimed;
      if (claimed === undefined) throw new HydrationError("portal content was not claimed", site);
      const { start, end } = claimed;
      const document = session.element.ownerDocument;
      const detached = document.createDocumentFragment();
      renderEffect(() => {
        move(start, end, mount?.() || document.body);
        onCleanup(() => move(start, end, detached));
      });
      if (claimed.layout.placement === "inert") {
        document.querySelector(`template[data-reze-portal="${plan.token}"]`)?.remove();
      }
    });
    return undefined;
  }) as JSX.Element;
}
