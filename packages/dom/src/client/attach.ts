import { bootIsland, disposeIslandsBetween, type BootOptions, type IslandDescriptor, type RangeLocator } from "./boot";

export interface StreamHandle {
  readonly done: Promise<void>;
  flush(isFinal: boolean): void;
  dispose(): void;
}

const Units = "template[data-rz-patch],script[data-rz-island],script[data-rz-end]";

function createLocator(target: Document): RangeLocator {
  const starts = new Map<string, Comment>();
  const scan = (): void => {
    starts.clear();
    const walker = target.createTreeWalker(target.documentElement, NodeFilter.SHOW_COMMENT);
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      const data = (node as Comment).data;
      if (data.startsWith("rz:")) starts.set(data.slice(3), node as Comment);
    }
  };
  const lookup = (token: string): { start: Comment; end: Comment } | undefined => {
    const start = starts.get(token);
    if (start === undefined || !start.isConnected) return undefined;
    const closing = `/rz:${token}`;
    for (let node = start.nextSibling; node !== null; node = node.nextSibling) {
      if (node.nodeType === 8 && (node as Comment).data === closing) return { start, end: node as Comment };
    }
    return undefined;
  };
  return {
    find(token) {
      return lookup(token) ?? (scan(), lookup(token));
    },
  };
}

export function attachStream(source: Document, target: Document, options: BootOptions = {}): StreamHandle {
  const locator = createLocator(target);
  const processed = new WeakSet<Element>();
  const done = Promise.withResolvers<void>();
  let finished = false;

  const apply = (unit: Element): void => {
    if (unit instanceof HTMLTemplateElement) {
      const token = unit.getAttribute("data-rz-patch")!;
      const range = locator.find(token);
      if (range === undefined) {
        console.error(`[reze] stream patch for unknown range ${token}`);
      } else {
        disposeIslandsBetween(range.start, range.end);
        for (let node = range.start.nextSibling; node !== null && node !== range.end; node = range.start.nextSibling) {
          node.parentNode!.removeChild(node);
        }
        range.end.parentNode!.insertBefore(target.importNode(unit.content, true), range.end);
      }
      unit.remove();
    } else if (unit.hasAttribute("data-rz-island")) {
      const token = unit.getAttribute("data-rz-island")!;
      bootIsland(token, JSON.parse(unit.textContent!) as IslandDescriptor, locator, options);
    } else {
      finished = true;
      observer.disconnect();
      done.resolve();
    }
  };

  const flush = (isFinal: boolean): void => {
    if (finished) return;
    for (const unit of source.querySelectorAll(Units)) {
      if (processed.has(unit) || (!isFinal && unit.nextSibling === null)) continue;
      processed.add(unit);
      apply(unit);
      if (finished) return;
    }
    if (isFinal) {
      finished = true;
      observer.disconnect();
      done.resolve();
    }
  };

  const observer = new MutationObserver(() => flush(false));
  if (source.documentElement !== null) observer.observe(source.documentElement, { childList: true, subtree: true });
  if (source === target && source.readyState !== "complete") {
    source.addEventListener("readystatechange", () => {
      if (source.readyState === "complete") flush(true);
    });
  }
  flush(source.readyState === "complete" && source === target);

  return {
    done: done.promise,
    flush,
    dispose() {
      finished = true;
      observer.disconnect();
      done.resolve();
    },
  };
}
