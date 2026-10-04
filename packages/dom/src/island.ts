import { onCleanup, signal } from "@rezejs/signals";

import { createComponent } from "./component";
import { insert } from "./insert";
import type { JSX } from "./jsx";
import { asyncComponent } from "./loading";

type Component<P> = (props: P) => JSX.Element;

export type IslandTrigger = "eager" | "idle" | "visible" | "media" | "interaction";

export interface IslandOptions {
  media?: string;
  rootMargin?: string;
}

const IdleFallbackMs = 200;
const IdleTimeoutMs = 2000;
const VisibleRootMargin = "200px";
const InteractionEvents = ["pointerdown", "focusin", "keydown"] as const;

function isPromiseLike<P>(value: Component<P> | PromiseLike<Component<P>>): value is PromiseLike<Component<P>> {
  return typeof (value as PromiseLike<Component<P>>).then === "function";
}

/**
 * A component that renders `fallback` and loads the real one when `trigger` fires:
 * `eager` loads at once, `idle` on the first idle period, `visible` when the shell
 * scrolls into view, `media` when its query matches, `interaction` on the first
 * pointer, focus or key event inside the shell. A failed load throws into the
 * surrounding error boundary. Only `visible` and `interaction` wrap the shell in an
 * element to observe; the rest render the fallback as is.
 */
export function island<P>(
  trigger: IslandTrigger,
  load: () => Component<P> | PromiseLike<Component<P>>,
  props: P,
  fallback?: () => JSX.Element,
  options?: IslandOptions,
): JSX.Element {
  const [started, start] = signal(trigger === "eager");
  if (trigger === "idle" || trigger === "media") armIsland(trigger, start, options);
  let view: (() => JSX.Element | undefined) | undefined;
  let direct: Component<P> | undefined;
  let shell: Node | undefined;
  const renderShell = (): Node | JSX.Element | undefined => {
    if (trigger !== "visible" && trigger !== "interaction") {
      return fallback?.();
    }
    if (shell === undefined) {
      const host = document.createElement("span");
      host.setAttribute("data-island", trigger);
      if (fallback !== undefined) {
        insert(host, fallback, null);
      } else {
        host.setAttribute("style", "display:block;min-width:1px;min-height:1px");
      }
      armIsland(trigger, start, options, host);
      shell = host;
    }
    return shell;
  };
  return () => {
    if (!started()) {
      return renderShell();
    }
    if (view === undefined && direct === undefined) {
      const loaded = load();
      if (isPromiseLike(loaded)) {
        view = asyncComponent(
          () =>
            Promise.resolve(loaded).then((component) => {
              if (process.env.NODE_ENV !== "production" && typeof component !== "function") {
                throw new Error("[reze] island: the loader did not return a component");
              }
              return [component] as const;
            }),
          (values) => createComponent(values()[0], props),
        );
      } else {
        direct = loaded;
        if (process.env.NODE_ENV !== "production" && typeof direct !== "function") {
          throw new Error("[reze] island: the loader did not return a component");
        }
      }
    }
    if (view !== undefined) {
      const current = view();
      return current === undefined ? fallback?.() : current;
    }
    return createComponent(direct as Component<P>, props);
  };
}

export function armIsland(trigger: IslandTrigger, start: (value: boolean) => unknown, options?: IslandOptions, host?: Element): void {
  if (trigger === "idle") {
    if (typeof requestIdleCallback === "function") {
      const handle = requestIdleCallback(() => start(true), { timeout: IdleTimeoutMs });
      onCleanup(() => cancelIdleCallback(handle));
    } else {
      const handle = setTimeout(() => start(true), IdleFallbackMs);
      onCleanup(() => clearTimeout(handle));
    }
  } else if (trigger === "media") {
    const query = matchMedia(options?.media ?? "");
    if (query.matches) {
      start(true);
    } else {
      const changed = (): void => {
        if (query.matches) {
          start(true);
          query.removeEventListener("change", changed);
        }
      };
      query.addEventListener("change", changed);
      onCleanup(() => query.removeEventListener("change", changed));
    }
  } else if (trigger === "visible" && host !== undefined) {
    const observer = new IntersectionObserver(
      entries => {
        if (entries.some(entry => entry.isIntersecting)) {
          observer.disconnect();
          start(true);
        }
      },
      { rootMargin: options?.rootMargin ?? VisibleRootMargin },
    );
    observer.observe(host);
    onCleanup(() => observer.disconnect());
  } else if (trigger === "interaction" && host !== undefined) {
    const fired = (): void => {
      start(true);
      for (const name of InteractionEvents) host.removeEventListener(name, fired, true);
    };
    for (const name of InteractionEvents) host.addEventListener(name, fired, true);
    onCleanup(() => {
      for (const name of InteractionEvents) host.removeEventListener(name, fired, true);
    });
  }
}
