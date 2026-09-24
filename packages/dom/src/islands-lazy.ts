import { hydrateNow, type IslandsState, type LazyIsland, type ServerIsland } from "./dom";

// oxlint-disable-next-line typescript/no-explicit-any
type Any = any;

/**
 * A lazy entry of the `hydrateIslands` map, emitted by the compiler for islands with an
 * `island:load` mode: `load()` resolves the island's module, its component is read off
 * `exportName`. `mode` is `idle`, `visible`, `interaction` or `eager`; any other waits a tick.
 */
export function lazyIsland(
  load: () => Promise<Record<string, Any>>,
  mode: string,
  exportName: string,
): LazyIsland {
  return {
    hydrate: (island, props, state, onPending) =>
      hydrateLazy(island, props, load, mode, exportName, state, onPending),
  };
}

function hydrateLazy(
  island: ServerIsland,
  props: Record<string, Any>,
  loadModule: () => Promise<Record<string, Any>>,
  mode: string,
  exportName: string,
  state: IslandsState,
  onPending: (cancel: () => void) => void,
): void {
  let settled = false;
  const cancels: (() => void)[] = [];
  const recorder = state.replay ? recordEvents(island) : undefined;
  const load = (): void => {
    if (settled || state.cancelled) return;
    settled = true;
    for (const cancel of cancels) cancel();
    void loadModule().then(
      (namespace) => {
        if (state.cancelled) return;
        const render = namespace[exportName];
        if (typeof render !== "function") {
          throw new Error(`hydrateIslands: no export "${exportName}" for island`);
        }
        hydrateNow(island, render, props);
        recorder?.replay();
      },
      () => recorder?.stop(),
    );
  };
  if (mode === "eager") {
    load();
    return;
  }
  cancels.push(watchInteraction(island, load));
  if (mode === "idle") {
    const idle = (window as Any).requestIdleCallback as
      | ((callback: () => void) => number)
      | undefined;
    if (idle) {
      const id = idle.call(window, load);
      const cancelIdle = (window as Any).cancelIdleCallback as ((id: number) => void) | undefined;
      cancels.push(() => (cancelIdle ? cancelIdle.call(window, id) : undefined));
    } else {
      const id = setTimeout(load, 1);
      cancels.push(() => clearTimeout(id));
    }
  } else if (mode === "visible") {
    cancels.push(watchVisible(island, load));
  } else if (mode !== "interaction") {
    const id = setTimeout(load, 1);
    cancels.push(() => clearTimeout(id));
  }
  onPending(() => {
    for (const cancel of cancels) cancel();
    recorder?.stop();
  });
}

/** The first element after the opening marker inside the island, else its parent. */
function visibleTarget(island: ServerIsland): Element | null {
  for (let n = island.open.nextSibling; n && n !== island.close; n = n.nextSibling) {
    if (n.nodeType === 1) return n as Element;
  }
  return island.open.parentNode as Element | null;
}

function watchVisible(island: ServerIsland, load: () => void): () => void {
  const Observer = (window as Any).IntersectionObserver as
    | (new (callback: (entries: { isIntersecting: boolean }[]) => void) => {
        observe(target: Element): void;
        disconnect(): void;
      })
    | undefined;
  if (!Observer) {
    const id = setTimeout(load, 1);
    return () => clearTimeout(id);
  }
  const observer = new Observer((entries) => {
    if (entries.some((entry) => entry.isIntersecting)) {
      observer.disconnect();
      load();
    }
  });
  const target = visibleTarget(island);
  if (target) observer.observe(target);
  else load();
  return () => observer.disconnect();
}

const ReplayedEvents = [
  "click",
  "input",
  "change",
  "submit",
  "keydown",
  "keyup",
  "pointerdown",
  "pointerup",
  "focusin",
  "focusout",
];
const ReplayedFields = [
  "bubbles",
  "cancelable",
  "composed",
  "detail",
  "view",
  "key",
  "code",
  "location",
  "repeat",
  "isComposing",
  "button",
  "buttons",
  "clientX",
  "clientY",
  "screenX",
  "screenY",
  "relatedTarget",
  "shiftKey",
  "ctrlKey",
  "altKey",
  "metaKey",
  "pointerId",
  "pointerType",
  "width",
  "height",
  "pressure",
  "isPrimary",
  "inputType",
  "data",
];
const MaxReplayedEvents = 32;

/** Whether the default action of `event` would leave the page before the island can handle it. */
function leavesPage(event: Event): boolean {
  if (event.type === "submit") return true;
  if (event.type !== "click") return false;
  const target = event.target as Element | null;
  return !!target?.closest?.('a[href], button[type="submit"], input[type="submit"]');
}

function replayed(event: Event): Event {
  const init: Record<string, unknown> = {};
  for (const field of ReplayedFields) {
    if (field in event) init[field] = (event as Any)[field];
  }
  return new (event.constructor as typeof Event)(event.type, init);
}

/** Records the delegated events inside `island` until `replay` dispatches them again or `stop`. */
function recordEvents(island: ServerIsland): { replay: () => void; stop: () => void } {
  const parent = island.open.parentNode as Element | null;
  const recorded: { event: Event; target: Element }[] = [];
  const onEvent = (event: Event): void => {
    const target = event.target as Element | null;
    if (!target || !inIslandRange(island, target)) return;
    if (leavesPage(event)) event.preventDefault();
    if (recorded.length === MaxReplayedEvents) recorded.shift();
    recorded.push({ event, target });
  };
  for (const name of ReplayedEvents) parent?.addEventListener(name, onEvent, true);
  const stop = (): void => {
    for (const name of ReplayedEvents) parent?.removeEventListener(name, onEvent, true);
  };
  const replay = (): void => {
    stop();
    for (const { event, target } of recorded.splice(0)) {
      if (!target.isConnected) continue;
      if (
        event.type === "submit" &&
        typeof (target as HTMLFormElement).requestSubmit === "function"
      ) {
        (target as HTMLFormElement).requestSubmit();
      } else {
        target.dispatchEvent(replayed(event));
      }
    }
  };
  return { replay, stop };
}

/**
 * Capture `pointerdown`/`focusin`/`keydown` on the opening marker's parent: an event inside the
 * island range loads it at once, in every lazy mode.
 */
function watchInteraction(island: ServerIsland, load: () => void): () => void {
  const parent = island.open.parentNode as Element | null;
  if (!parent) return () => {};
  const onEvent = (event: Event): void => {
    if (inIslandRange(island, event.target as Node | null)) load();
  };
  for (const name of ["pointerdown", "focusin", "keydown"]) {
    parent.addEventListener(name, onEvent, true);
  }
  return () => {
    for (const name of ["pointerdown", "focusin", "keydown"]) {
      parent.removeEventListener(name, onEvent, true);
    }
  };
}

function inIslandRange(island: ServerIsland, target: Node | null): boolean {
  for (let n = island.open.nextSibling; n && n !== island.close; n = n.nextSibling) {
    if (n === target || (n.nodeType === 1 && (n as Element).contains(target))) return true;
  }
  return false;
}
