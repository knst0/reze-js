import { getOwner } from "./context";
import { consumePendingSite } from "./internal/resource";
import { getActiveScope, getScopeObserver, scopeOfNode } from "./internal/scope";
import { IslandContext } from "./island";
import { useContext } from "./provide";

let next = 0;

/** An id that is a valid CSS identifier and unique on the page. */
export function createUniqueId(): string {
  const island = useContext(IslandContext);
  if (island !== undefined) return `${island.prefix}-${island.nextId++}`;
  if (__REZE_HTML__) {
    const site = consumePendingSite();
    const owner = getOwner();
    const id = getScopeObserver()?.resolveUniqueId?.({
      site,
      owner,
      scope: getActiveScope() ?? (owner !== undefined ? scopeOfNode(owner) : undefined),
    });
    if (id !== undefined) {
      return id;
    }
  }
  return "r" + next++;
}
