import { flush, render, type JSX } from "reze-js";

export interface Mounted {
  el: HTMLElement;
  dispose: () => void;
}

export interface Deferred<M> {
  load: () => Promise<M>;
  calls: () => number;
  resolve: (module: M) => void;
  reject: (error: Error) => void;
}

const live = new Set<() => void>();

/** Renders `code` into a fresh `<tag>` appended to the body; the returned `dispose` is idempotent. */
export function mount(code: () => JSX.Element, tag = "div"): Mounted {
  const el = document.createElement(tag);
  document.body.appendChild(el);
  const unmount = render(code, el);
  const dispose = (): void => {
    if (live.delete(dispose)) {
      unmount();
    }
  };
  live.add(dispose);
  return { el, dispose };
}

/** Disposes every live mount and empties the body; register with `afterEach(cleanup)`. */
export function cleanup(): void {
  for (const dispose of live) {
    dispose();
  }
  document.body.textContent = "";
}

export const tick: () => void = flush;

/** Yields one macrotask, letting pending promises and async components settle. */
export function settle(): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  return promise;
}

/** Controllable promise source for async tests; `reject` resets so `load` can be retried. */
export function deferred<M>(): Deferred<M> {
  let current = Promise.withResolvers<M>();
  let calls = 0;
  return {
    load: () => {
      calls++;
      return current.promise;
    },
    calls: () => calls,
    resolve: (module) => current.resolve(module),
    reject: (error) => {
      current.reject(error);
      current = Promise.withResolvers<M>();
    },
  };
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

/** Dispatches a bubbling, cancelable `type` event built with the constructor that matches it; returns the dispatch result. */
export function fire(target: Element, type: string, init: EventInit = {}): boolean {
  const options = { bubbles: true, cancelable: true, ...init };
  if (MouseTypes[type]) {
    return target.dispatchEvent(new MouseEvent(type, options));
  }
  if (KeyTypes[type]) {
    return target.dispatchEvent(new KeyboardEvent(type, options));
  }
  if (type.startsWith("focus") || type === "blur") {
    return target.dispatchEvent(new FocusEvent(type, options));
  }
  return target.dispatchEvent(new Event(type, options));
}
