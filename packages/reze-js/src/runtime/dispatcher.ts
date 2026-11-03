type Entry = unknown[];
type Handlers = Record<number, Record<string, (row: Entry) => (event: Event) => void>>;
type Factory = (entry: Entry, index: number, entries: Entry[]) => Handlers;
interface Chunk {
  default: Record<number, Factory>;
  g: number[];
}

const entries: Entry[] = [];
const urls: string[] = [];
const instances: Handlers[] = [];
const chunks: Record<string, Chunk> = {};
const loading: Record<string, Promise<void> | undefined> = {};
const eager: Record<string, number> = {};
const types = new Set<string>();
const queue: [Element[], Event][] = [];
const site = (node: EventTarget | null | undefined) => (node as Element | null | undefined)?.closest?.("[data-rz]");
const owner = (scope: number) => {
  while ((entries[scope]![0] as number) < 0) scope = ~(entries[scope]![0] as number);
  return scope;
};
const connection = (navigator as { connection?: { saveData?: boolean; effectiveType?: string } }).connection;
let dead: number | undefined;
let client: Promise<typeof import("virtual:rz-nav")> | undefined;
let bypass: HTMLFormElement | undefined;

const instance = (index: number) =>
  (instances[index] ??= chunks[urls[index]!]!.default[entries[index]![0] as number]!(entries[index]!, index, entries));

/** Instantiates the entries from `start` whose chunks are loaded and that start without an event. */
const start = (from = 0) =>
  entries.forEach((entry, index) => {
    const chunk = chunks[urls[index]!];
    if (index >= from && chunk && (entry[0] as number) >= 0 && (eager[urls[index]!] || chunk.g.includes(entry[0] as number)))
      instance(index);
  });

const run = (sites: Element[], event: Event) => {
  for (const node of sites) {
    const [scope, mark] = node
      .getAttribute("data-rz")!
      .split(".")
      .map((part) => parseInt(part, 36)) as [number, number];
    const handler = instance(owner(scope))[mark]?.[event.type];
    if (!handler) continue;
    Object.defineProperty(event, "currentTarget", { value: node, configurable: true });
    handler(entries[scope]!)(event);
    delete (event as { currentTarget?: unknown }).currentTarget;
    if (event.cancelBubble) break;
  }
};

const fail = (error: unknown) => {
  const queued = queue.splice(0);
  dead = 1;
  for (const [sites, event] of queued) {
    if (!event.defaultPrevented) continue;
    const target = event.target as HTMLElement;
    const link = sites[0]!.closest<HTMLAnchorElement>("a[href]");
    const submitter =
      ((event as SubmitEvent).submitter as HTMLButtonElement | null) ??
      target.closest?.<HTMLButtonElement | HTMLInputElement>("button:not([type]),[type=submit]");
    const form = submitter?.form ?? (event.type === "submit" ? (target as HTMLFormElement) : null);
    if (link) return location.assign(link.href);
    if (form) return form.requestSubmit(submitter);
  }
  if (
    dispatchEvent(new CustomEvent("rz:chunkerror", { cancelable: true, detail: { site: queued[0]?.[0][0], error } })) &&
    !sessionStorage["rz:reload"]
  ) {
    sessionStorage["rz:reload"] = 1;
    location.reload();
  }
};

const load = (url: string) =>
  (loading[url] ??= importChunk(url)
    .catch(() => importChunk(`${url}?r=1`))
    .then(
      (chunk) => {
        chunks[url] = chunk;
        start();
      },
      (error: unknown) => {
        loading[url] = undefined;
        throw error;
      },
    ));

const urlsOf = (sites: Element[]) => [...new Set(sites.map((node) => urls[owner(parseInt(node.getAttribute("data-rz")!, 36))]!))];

const handle = (event: Event) => {
  if (dead) return;
  const sites: Element[] = [];
  for (let node = site(event.target); node; node = site(node.parentElement)) {
    sites.push(node);
    for (const token of node.getAttribute("data-rz-prevent")?.split(" ") ?? []) {
      const [type, conditions] = token.split(":");
      if (type === event.type && (conditions ?? "").split("+").every((condition) => !condition || matches(event, condition))) {
        event.preventDefault();
      }
    }
  }
  if (!sites.length) return;
  const needed = urlsOf(sites).filter((url) => !chunks[url]);
  if (!needed.length) return run(sites, event);
  queue.push([sites, event]);
  sites[0]!.setAttribute("aria-busy", "true");
  Promise.all(needed.map(load))
    .then(() => {
      for (const [queued, replay] of queue.splice(0)) run(queued, replay);
    })
    .catch(fail)
    .finally(() => sites[0]!.removeAttribute("aria-busy"));
};

/** Adds a state block, `[events, chunk, eager, ...entries]`, whose entries follow the current ones. */
const add = ([events, url, isEager, ...block]: [string, string, number, ...Entry[]]) => {
  const base = entries.length;
  for (const entry of block) {
    if ((entry[0] as number) < 0) entry[0] = ~(~(entry[0] as number) + base);
    entries.push(entry);
    urls.push(url);
  }
  if (isEager) eager[url] = 1;
  for (const type of events.split(" ")) {
    if (type && !types.has(type)) {
      types.add(type);
      document.addEventListener(type, handle, true);
    }
  }
  if (chunks[url]) start(base);
  else if (isEager) load(url).catch(fail);
  else if (!connection?.saveData && !/2g/.test(connection?.effectiveType ?? "")) {
    (window.requestIdleCallback ?? setTimeout)(
      () =>
        [...document.querySelectorAll("[data-rz]")].some((node) => node.getBoundingClientRect().top < innerHeight) &&
        document.head.append(Object.assign(document.createElement("link"), { rel: "modulepreload", href: url, fetchPriority: "low" })),
    );
  }
};

(window as unknown as { rz: unknown }).rz = { n: () => entries.length, add };
dispatchEvent(new Event("rz:ready"));

const nav = () => (client ??= import("virtual:rz-nav"));

/** The same-origin link a navigation event targets, when the client may handle it. */
const link = (event: Event) => {
  const anchor = (event.target as Element | null)?.closest?.<HTMLAnchorElement>("a[href]");
  if (!anchor || anchor.origin !== location.origin || anchor.hasAttribute("download") || (anchor.target && anchor.target !== "_self"))
    return;
  if (anchor.pathname === location.pathname && anchor.search === location.search && anchor.hash) return;
  return anchor;
};

for (const type of ["pointerenter", "pointerdown", "focusin"]) {
  document.addEventListener(
    type,
    (event) => {
      if (type === "pointerenter" && !/mouse|pen/.test((event as PointerEvent).pointerType)) return;
      const urls = site(event.target) && urlsOf([site(event.target)!]);
      for (const url of urls || []) if (!chunks[url]) load(url).catch(() => {});
      const anchor = link(event);
      const form = (event.target as Element | null)?.closest?.("form");
      if (anchor && !connection?.saveData)
        nav().then(
          (m) => m.prefetch(anchor.href),
          () => {},
        );
      else if (form?.action.includes("_m=")) nav().catch(() => {});
    },
    true,
  );
}

addEventListener("click", (event) => {
  const anchor = link(event);
  if (!anchor || event.defaultPrevented || event.button || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  event.preventDefault();
  nav().then(
    (m) => m.navigate(anchor.href),
    () => location.assign(anchor.href),
  );
});

addEventListener("submit", (event) => {
  const form = event.target as HTMLFormElement;
  if (event.defaultPrevented || form === bypass || form.method !== "post" || !form.action.includes("_m=")) return;
  event.preventDefault();
  if (form.getAttribute("aria-busy")) return;
  const submitter = (event as SubmitEvent).submitter;
  form.setAttribute("aria-busy", "true");
  nav()
    .then((m) => m.submit(form, submitter))
    .catch(() => {
      bypass = form;
      form.requestSubmit(submitter);
    })
    .finally(() => form.removeAttribute("aria-busy"));
});

addEventListener("popstate", () => {
  nav().then(
    (m) => m.navigate(location.href, true),
    () => location.reload(),
  );
});

const block = document.querySelector("script[type=rz]");
if (block) {
  block.remove();
  add(JSON.parse(block.textContent!) as [string, string, number, ...Entry[]]);
}

/** A condition of `data-rz-prevent`: `Enter` for a key, or `field=value`, as in `ctrlKey=true`. */
function matches(event: Event, condition: string): boolean {
  const [field, value] = condition.includes("=") ? condition.split("=") : ["key", condition];
  return String((event as unknown as Record<string, unknown>)[field!]) === value;
}

/** Imports a route chunk. */
function importChunk(url: string): Promise<Chunk> {
  return import(/* @vite-ignore */ url) as Promise<Chunk>;
}
