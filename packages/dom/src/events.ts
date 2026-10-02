interface DelegateTarget extends Node {
  disabled?: boolean;
  host?: Node;
  [key: string]: unknown;
}

interface DelegatingDocument extends Document {
  $$events?: Set<string>;
}

type Handler = (this: Node, ...args: unknown[]) => void;

export const DelegatedEvents: Record<string, true> = {
  click: true,
  input: true,
  change: true,
  submit: true,
  keydown: true,
  keyup: true,
  pointerdown: true,
  pointerup: true,
  pointermove: true,
  focusin: true,
  focusout: true,
};

/** Installs one listener per event type on `doc`, which runs the `$$<type>` handlers from the target up to the root. */
export function delegateEvents(names: readonly string[], doc: Document = document): void {
  const installed = ((doc as DelegatingDocument).$$events ??= new Set());
  for (const name of names) {
    if (!installed.has(name)) {
      installed.add(name);
      doc.addEventListener(name, dispatchDelegated);
    }
  }
}

function dispatchDelegated(event: Event): void {
  const key = "$$" + event.type;
  const dataKey = key + "Data";
  const target = event.target as DelegateTarget | null;
  let node: DelegateTarget | null | undefined;
  if (target !== null && target.getRootNode() === event.currentTarget) {
    node = target;
  } else {
    node = (event.composedPath()[0] ?? event.target) as DelegateTarget | null | undefined;
  }
  Object.defineProperty(event, "currentTarget", { configurable: true, get: () => node ?? document });
  while (node) {
    const handler = node[key] as Handler | undefined;
    if (handler && !node.disabled) {
      const data = node[dataKey];
      if (data === undefined) {
        handler.call(node, event);
      } else {
        handler.call(node, data, event);
      }
      if (event.cancelBubble) {
        return;
      }
    }
    node = (node.parentNode ?? node.host) as DelegateTarget | null | undefined;
  }
}

/**
 * With `delegate`, stores `handler` as `$$<name>` for {@link delegateEvents}; a `[handler, data]` pair calls
 * `handler(data, event)`. Otherwise adds a direct listener; a `[handler, options]` pair passes `options`.
 */
export function addEventListener(node: Element, name: string, handler: unknown, delegate?: boolean): void {
  if (delegate) {
    const target = node as unknown as DelegateTarget;
    if (Array.isArray(handler)) {
      target["$$" + name] = handler[0];
      target["$$" + name + "Data"] = handler[1];
    } else {
      target["$$" + name] = handler;
    }
  } else if (Array.isArray(handler)) {
    if (handler[0]) {
      node.addEventListener(name, handler[0] as EventListener, handler[1] as AddEventListenerOptions | undefined);
    }
  } else if (handler) {
    node.addEventListener(name, handler as EventListener);
  }
}
