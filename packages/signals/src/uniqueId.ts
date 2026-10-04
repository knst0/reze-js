import { getOwner } from "./context";
import { consumePendingSite } from "./internal/resource";
import { currentModuleId, getActiveScope, getScopeObserver, scopeOfNode } from "./internal/scope";

let next = 0;

/** An id that is a valid CSS identifier and unique on the page. */
export function createUniqueId(): string {
  if (__REZE_HTML__ || __REZE_HYDRATE__) {
    const site = consumePendingSite();
    const owner = getOwner();
    const id = getScopeObserver()?.resolveUniqueId?.({
      site,
      moduleId: currentModuleId() ?? site?.module,
      owner,
      scope: getActiveScope() ?? (owner !== undefined ? scopeOfNode(owner) : undefined),
    });
    if (id !== undefined) {
      return id;
    }
  }
  return "r" + next++;
}
