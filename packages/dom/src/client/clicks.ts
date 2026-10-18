import { elementsOf } from "./forms";

const AnyElement = "*";

export interface HeldClick {
  readonly target: Element;
  readonly init: MouseEventInit;
}

export interface ClickPosition {
  readonly index: number;
  readonly tag: string;
}

export function holdClicks(nodes: readonly Node[]): () => HeldClick | undefined {
  const hosts = nodes.filter((node): node is Element => node.nodeType === 1);
  let held: HeldClick | undefined;
  const hold = (event: MouseEvent): void => {
    held = {
      target: event.target as Element,
      init: {
        bubbles: true,
        cancelable: true,
        composed: true,
        detail: event.detail,
        screenX: event.screenX,
        screenY: event.screenY,
        clientX: event.clientX,
        clientY: event.clientY,
        ctrlKey: event.ctrlKey,
        shiftKey: event.shiftKey,
        altKey: event.altKey,
        metaKey: event.metaKey,
        button: event.button,
        buttons: event.buttons,
      },
    };
    event.preventDefault();
    event.stopPropagation();
  };
  for (const host of hosts) host.addEventListener("click", hold, true);
  return () => {
    for (const host of hosts) host.removeEventListener("click", hold, true);
    return held;
  };
}

export function clickPosition(nodes: readonly Node[], target: Element): ClickPosition | undefined {
  const index = elementsOf(nodes, AnyElement).indexOf(target);
  return index === -1 ? undefined : { index, tag: target.localName };
}

export function elementAt(nodes: readonly Node[], position: ClickPosition): Element | undefined {
  const element = elementsOf(nodes, AnyElement)[position.index];
  return element?.localName === position.tag ? element : undefined;
}

export function replayClick(target: Element | undefined, held: HeldClick | undefined): void {
  if (target === undefined || held === undefined) return;
  target.dispatchEvent(new MouseEvent("click", held.init));
}
