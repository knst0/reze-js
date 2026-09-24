import { render } from "@rezejs/dom";
import type { JSX } from "@rezejs/dom/jsx-runtime";
import { flushSync } from "@rezejs/signals";

/** A mounted tree plus its disposer. Register specs with `afterEach(cleanup)`. */
export interface Mounted {
  el: HTMLElement;
  dispose: () => void;
}

const live = new Set<() => void>();

/**
 * Renders `code` into a fresh `<tag>` appended to the body. Disposers run via
 * `cleanup()`; calling the returned `dispose` early is safe.
 */
export function mount(code: () => JSX.Element, tag = "div"): Mounted {
  const el = document.createElement(tag);
  document.body.appendChild(el);
  const dispose = render(code, el);
  let disposed = false;
  const once = (): void => {
    if (!disposed) {
      disposed = true;
      live.delete(once);
      dispose();
    }
  };
  live.add(once);
  return { el, dispose: once };
}

/** Disposes every undisposed mount and empties the body. */
export function cleanup(): void {
  for (const dispose of [...live]) {
    dispose();
  }
  document.body.textContent = "";
}

/** Flushes pending effects now; use after `fire` to read the DOM synchronously. */
export function tick(): void {
  flushSync();
}

const MouseTypes: Record<string, true> = {
  click: true,
  dblclick: true,
  mousedown: true,
  mouseup: true,
  mousemove: true,
  mouseover: true,
  mouseout: true,
  mouseenter: true,
  mouseleave: true,
  contextmenu: true,
};
const KeyTypes: Record<string, true> = { keydown: true, keyup: true, keypress: true };

/**
 * Dispatches `type` on `target` with bubbling on, picking the event constructor
 * by type, and returns the dispatch result.
 */
export function fire(target: Element, type: string, init: EventInit = {}): boolean {
  const base = { bubbles: true, cancelable: true, ...init };
  if (MouseTypes[type]) {
    return target.dispatchEvent(new MouseEvent(type, base));
  }
  if (KeyTypes[type]) {
    return target.dispatchEvent(new KeyboardEvent(type, base));
  }
  if (type.startsWith("focus") || type === "blur") {
    return target.dispatchEvent(new FocusEvent(type, base));
  }
  return target.dispatchEvent(new Event(type, base));
}
