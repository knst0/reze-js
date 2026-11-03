import type { Rz } from "./client";

/** The dispatcher's state, with its chunk loader and its failure ladder. */
export interface Dispatcher extends Rz {
  l: (module: number) => Promise<void>;
  f: (error: unknown) => void;
}

const connection = (navigator as { connection?: { saveData?: boolean; effectiveType?: string } }).connection;
const preloaded = new Set<string>();
let seen: IntersectionObserver | undefined;

/**
 * Loads the chunk of a component that starts without an event once one of its sites enters the
 * viewport, or at once when the component has no site, and preloads the chunk of any other site
 * in the viewport with a low priority, once the browser is idle after a state block.
 */
export function watch(rz: Dispatcher, urls: string[]): void {
  (window.requestIdleCallback ?? setTimeout)(() => observe(rz, urls));
}

function observe(rz: Dispatcher, urls: string[]): void {
  const { e: entries, u: modules, c: chunks } = rz;
  const owner = (node: Element) => {
    let index = parseInt(node.getAttribute("data-rz")!, 36);
    while ((entries[index]![0] as number) < 0) index = ~(entries[index]![0] as number);
    return index;
  };
  seen?.disconnect();
  seen = new IntersectionObserver((records) => {
    for (const { target, isIntersecting } of records) {
      const index = owner(target);
      const module = modules[index]!;
      const url = urls[module]!;
      if (!isIntersecting || chunks[module]) continue;
      seen!.unobserve(target);
      if (rz.s.includes(entries[index]![0] as number)) rz.l(module).catch(rz.f);
      else if (!preloaded.has(url) && !connection?.saveData && !/2g/.test(connection?.effectiveType ?? "")) {
        preloaded.add(url);
        document.head.append(Object.assign(document.createElement("link"), { rel: "modulepreload", href: url, fetchPriority: "low" }));
      }
    }
  });
  const sited = new Set<number>();
  for (const node of document.querySelectorAll("[data-rz]")) {
    sited.add(owner(node));
    seen.observe(node);
  }
  entries.forEach((entry, index) => {
    if (rz.s.includes(entry[0] as number) && !chunks[modules[index]!] && !sited.has(index)) rz.l(modules[index]!).catch(rz.f);
  });
}
