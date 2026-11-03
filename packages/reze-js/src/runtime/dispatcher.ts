import type { Chunk } from "./client";

type Block = [string, number | Record<number, number>, number[], ...unknown[][]];

const entries: unknown[][] = [];
const modules: number[] = [];
const chunks: Record<number, Chunk> = {};
const loading: Record<number, Promise<void> | 0> = {};
const queue: [Element[], Event][] = [];
const up = (node: unknown, selector: string) => (node as Element | null | undefined)?.closest?.(selector) as any;
const noop = () => {};
let run: Chunk["r"] | undefined;
let dead: unknown;

/** The build manifest: the URLs of the client chunks by index, and the idle preload. */
const manifest = import("virtual:rz-chunks");

/** Imports the chunk at `url`, then once more past the cache. */
const get = (url: string) => import(/* @vite-ignore */ url).catch(() => import(/* @vite-ignore */ url + "?r=1")) as Promise<Chunk>;

const load = (module: number) =>
  (loading[module] ||= manifest
    .then((urls) => get(urls.default[module]!))
    .then(
      (chunk) => {
        (run = (chunks[module] = chunk).r)(rz);
      },
      (error: unknown) => {
        loading[module] = 0;
        throw error;
      },
    ));

/**
 * Falls back to the native action of the first prevented queued event, a submit or the click of a
 * link or a submit button, or reports `error`.
 */
const fail = (error: unknown) => {
  const queued = queue.splice(0);
  dead = 1;
  for (const [, event] of queued) {
    const action = up(event.target, "a[href],button:not([type]),[type=submit]");
    if (event.defaultPrevented && event.type == "submit")
      return (event.target as HTMLFormElement).requestSubmit((event as SubmitEvent).submitter);
    if (event.defaultPrevented && action) return action.click();
  }
  if (
    dispatchEvent(new CustomEvent("rz:chunkerror", { cancelable: true, detail: { site: queued[0]?.[0][0], error } })) &&
    !sessionStorage["rz:reload"]
  ) {
    sessionStorage["rz:reload"] = 1;
    location.reload();
  }
};

/** The chunk of the handler site `node`, by its index in the build manifest. */
const moduleOf = (node: Element) => modules[parseInt(node.getAttribute("data-rz")!, 36)]!;

/** Runs the handler sites around the target of `event`, once their chunks have loaded. */
const handle = (event: Event) => {
  const sites: Element[] = [];
  for (let node: Element | undefined = up(event.target, "[data-rz]"); node && !dead; node = up(node.parentElement, "[data-rz]")) {
    sites.push(node);
    for (const token of node.getAttribute("data-rz-prevent")?.split(" ") ?? []) {
      const [type, ...conditions] = token.split(/[:+]/);
      if (
        type == event.type &&
        conditions.every((condition) => {
          const [field, value] = condition.split("=");
          return (event as unknown as Record<string, unknown>)[value ? field! : "key"] + "" == (value ?? field);
        })
      )
        event.preventDefault();
    }
  }
  const needed = sites.map(moduleOf).filter((module) => !chunks[module]);
  if (!sites[0]) return;
  if (!needed.length) return run!(rz, sites, event);
  queue.push([sites, event]);
  sites[0].setAttribute("aria-busy", "true");
  Promise.all(needed.map(load))
    .then(() => {
      for (const [queued, replay] of queue.splice(0)) run!(rz, queued, replay);
    })
    .catch(fail)
    .finally(() => sites[0]!.removeAttribute("aria-busy"));
};

/**
 * Adds a state block, `[events, chunks, eager, ...entries]`, whose entries follow the current ones.
 * `chunks` is the manifest index of the page's chunk, or of each factory's chunk by factory id, and
 * `eager` lists the factories that start without an event.
 */
const add = ([events, chunk, starting, ...block]: Block) => {
  const base = entries.length;
  rz.s.push(...starting);
  for (const entry of block) {
    const id = (entry[0] as number) < 0 ? (entry[0] = ~(~(entry[0] as number) + base)) : (entry[0] as number);
    modules[entries.push(entry) - 1] = id < 0 ? modules[~id]! : ((chunk as Record<number, number>)[id] ?? (chunk as number));
  }
  for (const type of events.split(" ")) document.addEventListener(type, handle, true);
  run?.(rz);
  manifest.then((m) => m.watch(rz), noop);
};

const rz = ((window as unknown as { rz: unknown }).rz = {
  e: entries,
  u: modules,
  c: chunks,
  g: get,
  s: [] as number[],
  l: load,
  f: fail,
  add,
});

for (const type of ["pointerenter", "pointerdown", "focusin"]) {
  document.addEventListener(
    type,
    (event) => {
      const node = up(event.target, "[data-rz]");
      if (node && (type != "pointerenter" || /mouse|pen/.test((event as PointerEvent).pointerType))) load(moduleOf(node)).catch(noop);
    },
    true,
  );
}

const block = document.querySelector("script[type=rz]");
if (block) {
  block.remove();
  add(JSON.parse(block.textContent!) as Block);
}
