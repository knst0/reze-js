import { HeadManagedAttribute, mergeStreamedHead } from "../head";
import { bootIsland, disposeIslandsBetween, type BootOptions, type IslandDescriptor, type RangeLocator } from "./boot";

export interface StreamHandle {
  readonly done: Promise<void>;
  flush(isFinal: boolean): void;
  dispose(): void;
}

export interface AttachOptions extends BootOptions {
  /** Tokens a swap already resolved: parts of a kept layout or a persisted element it replaced with the live node. */
  readonly skipToken?: (token: string) => boolean;
  /** Runs after a flush that applied at least one patch, head tag or island descriptor. */
  readonly onApplied?: () => void;
}

const Units = `template[data-rz-patch],template[${HeadManagedAttribute}],script[data-rz-island],script[data-rz-end]`;

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

export function attachStream(source: Document, target: Document, options: AttachOptions = {}): StreamHandle {
  const locator = createLocator(target);
  const processed = new WeakSet<Element>();
  const done = Promise.withResolvers<void>();
  let finished = false;

  let applied = false;
  const apply = (unit: Element): void => {
    applied = true;
    if (unit instanceof HTMLTemplateElement && unit.hasAttribute(HeadManagedAttribute)) {
      mergeStreamedHead(target, unit);
      unit.remove();
    } else if (unit instanceof HTMLTemplateElement) {
      const token = unit.getAttribute("data-rz-patch")!;
      if (options.skipToken?.(token) === true) {
        unit.remove();
        return;
      }
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
      if (options.skipToken?.(token) !== true) bootIsland(token, JSON.parse(unit.textContent!) as IslandDescriptor, locator, options);
    } else {
      finished = true;
      observer.disconnect();
      done.resolve();
    }
  };

  const flush = (isFinal: boolean): void => {
    if (finished) return;
    applied = false;
    for (const unit of source.querySelectorAll(Units)) {
      if (processed.has(unit) || (!isFinal && unit.nextSibling === null)) continue;
      processed.add(unit);
      apply(unit);
      if (finished) return;
    }
    if (applied) options.onApplied?.();
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

/** Fetches the streamed parts a prerendered shell left out and applies them to the live document. */
export async function resumeStream(url: string, options: AttachOptions = {}): Promise<boolean> {
  let response: Response;
  try {
    response = await fetch(url, {
      credentials: "same-origin",
      headers: { accept: "text/html", "x-reze-resume": "1" },
    });
  } catch {
    return false;
  }
  if (!response.ok || response.headers.get("x-reze-resume") !== "1") return false;
  const incoming = document.implementation.createHTMLDocument("");
  incoming.open();
  const handle = attachStream(incoming, document, options);
  try {
    if (response.body === null) {
      incoming.write(await response.text());
    } else {
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        incoming.write(decoder.decode(value, { stream: true }));
      }
      incoming.write(decoder.decode());
    }
    incoming.close();
    handle.flush(true);
    return true;
  } catch {
    handle.dispose();
    return false;
  }
}
