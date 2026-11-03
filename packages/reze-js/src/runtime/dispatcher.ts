type Entry = unknown[];
type Handlers = Record<number, Record<string, (row: Entry) => (event: Event) => void>>;
type Factory = (entry: Entry, index: number, entries: Entry[]) => Handlers;
interface Chunk {
  default: Record<number, Factory>;
  g: number[];
}

const block = document.querySelector("script[type=rz]");

if (block) {
  const [events, url, eager, ...entries] = JSON.parse(block.textContent!) as [string, string, number, ...Entry[]];
  const instances: Handlers[] = [];
  const queue: [Element[], Event][] = [];
  const site = (node: EventTarget | null | undefined) => (node as Element | null | undefined)?.closest?.("[data-rz]");
  let chunk: Chunk | undefined;
  let loading: Promise<void> | undefined;
  let dead: number | undefined;

  const instance = (index: number) => (instances[index] ??= chunk!.default[entries[index]![0] as number]!(entries[index]!, index, entries));

  const run = (sites: Element[], event: Event) => {
    for (const node of sites) {
      const [scope, mark] = node
        .getAttribute("data-rz")!
        .split(".")
        .map((part) => parseInt(part, 36)) as [number, number];
      let owner = scope;
      while ((entries[owner]![0] as number) < 0) owner = ~(entries[owner]![0] as number);
      const handler = instance(owner)[mark]?.[event.type];
      if (!handler) continue;
      Object.defineProperty(event, "currentTarget", { value: node, configurable: true });
      handler(entries[scope]!)(event);
      delete (event as { currentTarget?: unknown }).currentTarget;
      if (event.cancelBubble) break;
    }
  };

  const fail = (error: unknown) => {
    const queued = queue.splice(0);
    if (!queued.length && !eager) return;
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

  const load = () =>
    (loading ??= importChunk(url)
      .catch(() => importChunk(`${url}?r=1`))
      .then(
        (loaded) => {
          chunk = loaded;
          entries.forEach(
            (entry, index) => (entry[0] as number) >= 0 && (eager || loaded.g.includes(entry[0] as number)) && instance(index),
          );
          for (const [sites, event] of queue.splice(0)) run(sites, event);
        },
        (error: unknown) => {
          loading = undefined;
          throw error;
        },
      ));

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
    if (chunk) return run(sites, event);
    queue.push([sites, event]);
    sites[0]!.setAttribute("aria-busy", "true");
    load()
      .catch(fail)
      .finally(() => sites[0]!.removeAttribute("aria-busy"));
  };

  const intent = (event: Event) =>
    (event.type !== "pointerenter" || /mouse|pen/.test((event as PointerEvent).pointerType)) &&
    site(event.target) &&
    load().catch(() => {});

  for (const type of events.split(" ")) document.addEventListener(type, handle, true);
  for (const type of ["pointerenter", "pointerdown", "focusin"]) document.addEventListener(type, intent, true);

  const connection = (navigator as { connection?: { saveData?: boolean; effectiveType?: string } }).connection;
  if (eager) load().catch(fail);
  else if (!connection?.saveData && !/2g/.test(connection?.effectiveType ?? "")) {
    (window.requestIdleCallback ?? setTimeout)(
      () =>
        [...document.querySelectorAll("[data-rz]")].some((node) => node.getBoundingClientRect().top < innerHeight) &&
        document.head.append(Object.assign(document.createElement("link"), { rel: "modulepreload", href: url, fetchPriority: "low" })),
    );
  }
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
