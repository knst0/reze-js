import { signal } from "@rezejs/signals";

import { createComponent } from "../component";
import { armIsland, island, type IslandOptions, type IslandTrigger } from "../island";
import type { JSX } from "../jsx";
import { asyncComponent } from "../loading";
import { prepareAsyncComponent, prepareComponent } from "./flows";
import { claimRoot, prepareAppend, queueAttr } from "./native";
import type { Site } from "./protocol";
import { managedRange } from "./range";
import { preparingSession } from "./session";

type Component<P> = (props: P) => JSX.Element;

export function prepareIsland<P>(site: Site, trigger: IslandTrigger, load: () => Component<P> | PromiseLike<Component<P>>, props: P, fallback?: () => JSX.Element, options?: IslandOptions): JSX.Element {
  const session = preparingSession();
  if (session === undefined) return island(trigger, load, props, fallback, options);
  return managedRange(session, "island", "island", site, () => {
    if (trigger === "eager") {
      const loaded = load();
      if (typeof loaded === "function") return prepareComponent(site, loaded, props);
      return prepareAsyncComponent(site, () => Promise.resolve(loaded).then(component => [component]), values => prepareComponent(site, values()[0]!, props));
    }
    let host: Element | undefined;
    let shell: JSX.Element;
    if (trigger === "visible" || trigger === "interaction") {
      host = claimRoot(site, "span");
      queueAttr(host, site, "data-island", trigger);
      if (fallback === undefined) queueAttr(host, site, "style", "display:block;min-width:1px;min-height:1px");
      else prepareAppend(host, site, 0, fallback);
      shell = host;
    } else shell = fallback?.();
    const [started, start] = signal(false);
    session.deferCommit(() => armIsland(trigger, start, options, host));
    let view: (() => JSX.Element | undefined) | undefined;
    let direct: Component<P> | undefined;
    return () => {
      if (!started()) return shell;
      if (view === undefined && direct === undefined) {
        const loaded = load();
        if (typeof loaded === "function") direct = loaded;
        else view = asyncComponent(() => Promise.resolve(loaded).then(component => [component]), values => createComponent(values()[0]!, props));
      }
      if (view !== undefined) {
        const current = view();
        return current === undefined ? shell : current;
      }
      return createComponent(direct!, props);
    };
  }) as JSX.Element;
}
