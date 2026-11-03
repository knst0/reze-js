import type { Chunk } from "./client";

type Block = [string, string | Record<number, string>, number[], ...unknown[][]];

const entries: unknown[][] = [];
const urls: string[] = [];
const chunks: Record<string, Chunk> = {};
const loading: Record<string, Promise<void> | 0> = {};
const queue: [Element[], Event][] = [];
const up = (node: unknown, selector: string) => (node as Element | null | undefined)?.closest?.(selector) as any;
const noop = () => {};
let run: Chunk["r"] | undefined;
let dead: unknown;

const get = (url: string) => import(/* @vite-ignore */ url) as Promise<Chunk>;

const load = (url: string) =>
  (loading[url] ||= get(url)
    .catch(() => get(url + "?r=1"))
    .then(
      (chunk) => {
        (run = (chunks[url] = chunk).r)(rz);
      },
      (error: unknown) => {
        loading[url] = 0;
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

/** The chunk URL of the handler site `node`. */
const urlOf = (node: Element) => urls[parseInt(node.getAttribute("data-rz")!, 36)]!;

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
          return String((event as unknown as Record<string, unknown>)[value ? field! : "key"]) == (value ?? field);
        })
      )
        event.preventDefault();
    }
  }
  const needed = sites.map(urlOf).filter((url) => !chunks[url]);
  if (!sites[0]) return;
  if (!needed[0]) return run!(rz, sites, event);
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
 * `chunks` is the URL of the page's chunk, or the URL of each factory's chunk by factory id, and
 * `eager` lists the factories that start without an event.
 */
const add = ([events, chunk, starting, ...block]: Block) => {
  const base = entries.length;
  rz.s.push(...starting);
  for (const entry of block) {
    const id = (entry[0] as number) < 0 ? (entry[0] = ~(~(entry[0] as number) + base)) : (entry[0] as number);
    urls[entries.push(entry) - 1] = id < 0 ? urls[~id]! : typeof chunk == "string" ? chunk : chunk[id]!;
  }
  for (const type of events.split(" ")) document.addEventListener(type, handle, true);
  run?.(rz);
  import("virtual:rz-idle").then((m) => m.watch(rz), noop);
};

const rz = ((window as unknown as { rz: unknown }).rz = {
  e: entries,
  u: urls,
  c: chunks,
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
      if (node && (type != "pointerenter" || /mouse|pen/.test((event as PointerEvent).pointerType))) load(urlOf(node)).catch(noop);
    },
    true,
  );
}

const block = document.querySelector("script[type=rz]");
if (block) {
  block.remove();
  add(JSON.parse(block.textContent!) as Block);
}
