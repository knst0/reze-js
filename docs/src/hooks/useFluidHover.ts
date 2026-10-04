import { onCleanup } from "reze-js";

/** Attaches nearest-row hover and gap clicks to a static vertical group of links. */
export function useFluidHover() {
  let container: HTMLElement | undefined;
  let highlight: HTMLElement | null = null;
  let items: NodeListOf<HTMLAnchorElement> | undefined;
  let boxes = new Float64Array(0);
  let top = 0;
  let scaleY = 1;
  let activeIndex = -1;
  let needsMeasure = true;
  let observer: ResizeObserver | undefined;

  const hide = () => {
    if (activeIndex !== -1) items?.[activeIndex]?.removeAttribute("data-fluid-hover-active");
    activeIndex = -1;
    container?.removeAttribute("data-fluid-hovering");
  };

  const invalidate = () => {
    needsMeasure = true;
    hide();
  };

  const measure = () => {
    if (!container) return;
    highlight = container.querySelector<HTMLElement>("[data-fluid-hover-highlight]");
    items = container.querySelectorAll<HTMLAnchorElement>("a[href]");
    if (boxes.length !== items.length * 4) boxes = new Float64Array(items.length * 4);
    const bounds = container.getBoundingClientRect();
    top = bounds.top;
    scaleY = container.offsetHeight ? bounds.height / container.offsetHeight : 1;
    for (let i = 0; i < items.length; i++) {
      const item = items[i]!;
      const offset = i * 4;
      boxes[offset] = item.offsetLeft;
      boxes[offset + 1] = item.offsetTop;
      boxes[offset + 2] = item.offsetWidth;
      boxes[offset + 3] = item.offsetHeight;
    }
    needsMeasure = false;
  };

  const pick = (clientY: number) => {
    if (needsMeasure) measure();
    if (!items || !highlight || !container || !scaleY) return;
    const y = (clientY - top) / scaleY;
    let nearest = -1;
    let distance = Infinity;
    for (let i = 0; i < items.length; i++) {
      const offset = i * 4;
      if (!boxes[offset + 3]) continue;
      const delta = Math.abs(y - boxes[offset + 1]! - boxes[offset + 3]! / 2);
      if (delta < distance) {
        distance = delta;
        nearest = i;
      }
    }
    if (nearest === activeIndex) return;
    const wasHidden = activeIndex === -1;
    hide();
    if (nearest === -1) return;
    activeIndex = nearest;
    const offset = nearest * 4;
    if (wasHidden) highlight.style.transition = "none";
    highlight.style.transform = `translate(${boxes[offset]}px, ${boxes[offset + 1]}px)`;
    highlight.style.width = `${boxes[offset + 2]}px`;
    highlight.style.height = `${boxes[offset + 3]}px`;
    if (wasHidden) {
      highlight.getBoundingClientRect();
      highlight.style.removeProperty("transition");
    }
    items[nearest]!.setAttribute("data-fluid-hover-active", "");
    container.setAttribute("data-fluid-hovering", "");
  };

  const onPointerEnter = (event: PointerEvent) => {
    if (event.pointerType !== "mouse") return;
    needsMeasure = true;
    pick(event.clientY);
  };

  const onPointerMove = (event: PointerEvent) => {
    if (event.pointerType === "mouse") pick(event.clientY);
  };

  const onClick = (event: MouseEvent) => {
    if (
      event.defaultPrevented ||
      event.button !== 0 ||
      event.detail === 0 ||
      event.metaKey ||
      event.ctrlKey ||
      event.altKey ||
      event.shiftKey
    )
      return;
    if ((event.target as Element).closest("a, button, input, select, textarea")) return;
    if (activeIndex === -1) return;
    pick(event.clientY);
    const item = items?.[activeIndex];
    if (!item) return;
    event.preventDefault();
    item.click();
  };

  const detach = () => {
    if (container === undefined) return;
    observer?.disconnect();
    container?.removeEventListener("pointerenter", onPointerEnter);
    container?.removeEventListener("pointermove", onPointerMove);
    container?.removeEventListener("pointerleave", hide);
    container?.removeEventListener("pointercancel", hide);
    container?.removeEventListener("click", onClick);
    window.removeEventListener("scroll", invalidate, true);
    container = undefined;
    items = undefined;
    highlight = null;
  };

  onCleanup(detach);

  return (element: HTMLElement) => {
    detach();
    container = element;
    needsMeasure = true;
    observer = new ResizeObserver(invalidate);
    observer.observe(element);
    element.addEventListener("pointerenter", onPointerEnter);
    element.addEventListener("pointermove", onPointerMove);
    element.addEventListener("pointerleave", hide);
    element.addEventListener("pointercancel", hide);
    element.addEventListener("click", onClick);
    window.addEventListener("scroll", invalidate, true);
  };
}
