import { build, keys, routes } from "virtual:rz-routes";

import { MutationInputError, type Issue } from "./errors";
import { compare, match } from "./match";
import { morph } from "./morph";

/** The dispatcher's state blocks: the count of entries, and the call that adds a block. */
interface Dispatcher {
  n(): number;
  add(block: unknown[]): void;
}

interface Fetched {
  status: number;
  /** The depth of the segment the body renders, `0` for the whole page. */
  depth: number;
  url: string;
  redirected: boolean;
  text: string;
}

type Kind = "navigation" | "mutation";

const rz = (window as unknown as { rz: Dispatcher }).rz;
const ordered = [...routes].sort(compare);
const prefetched = new Map<string, [number, Promise<Fetched>]>();
let current = new URL(location.href);
let send = (request: Request): Promise<Response> => fetch(request);

history.scrollRestoration = "manual";

/** The key of each segment of the page at `url`: its module, params, and read search params. */
function segments(url: URL): string[] | undefined {
  const parts = url.pathname.split("/").filter(Boolean);
  for (const route of ordered) {
    const matched = match(route, parts);
    if (!matched) continue;
    return route.chain.map((node) => {
      const [names, search, path] = keys[node]!;
      return [node, ...names.map((name) => matched[0][name] ?? ""), search ? url.search : "", path ? url.pathname : ""].join("\0");
    });
  }
}

/** The depth of the first segment that changes from `from` to `to`, or `-1` when none does. */
function changed(from: URL, to: URL): number {
  const before = segments(from);
  const after = segments(to);
  if (!before || !after) return 0;
  let depth = 0;
  while (depth < after.length && before[depth] === after[depth]) depth++;
  if (depth < after.length) return depth;
  return from.href === to.href ? after.length - 1 : -1;
}

async function request(url: URL, depth: number, init: RequestInit = {}): Promise<Fetched> {
  const target = new URL(url);
  if (depth > 0) target.searchParams.set("_s", String(depth));
  target.searchParams.set("_b", build);
  const response = await send(new Request(target, init));
  if (response.status === 409 || !response.headers.get("content-type")?.startsWith("text/html")) throw new Error(String(response.status));
  return {
    status: response.status,
    depth: Number(response.headers.get("rz-segment") ?? 0),
    url: response.redirected ? response.url : url.href,
    redirected: response.redirected,
    text: await response.text(),
  };
}

function load(url: URL, depth: number): Promise<Fetched> {
  const key = `${url.href} ${depth}`;
  const hit = prefetched.get(key);
  prefetched.delete(key);
  return hit && hit[0] > Date.now() ? hit[1] : request(url, depth);
}

/** The live nodes of segment `depth`: the body for `0`, or the nodes between its markers. */
function range(depth: number): [Node, Node[], Node | null] | undefined {
  if (depth === 0) return [document.body, [...document.body.childNodes], null];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_COMMENT);
  while (walker.nextNode()) {
    const start = walker.currentNode as Comment;
    if (start.data !== `s${depth}`) continue;
    const nodes: Node[] = [];
    let open = 0;
    for (let node = start.nextSibling; node; node = node.nextSibling) {
      if (node.nodeType === 8) {
        const data = (node as Comment).data;
        if (data === "/" && open-- === 0) return [start.parentNode!, nodes, node];
        if (/^s\d+$/.test(data)) open++;
      }
      nodes.push(node);
    }
  }
}

const offset = (base: number) => (id: string) => id.replace(/^[0-9a-z]+/, (scope) => (parseInt(scope, 36) + base).toString(36));

/** Applies a fetched page through the cancelable `rz:swap` event, then runs `after`. */
function swap(fetched: Fetched, kind: Kind, after: (root: Node[]) => void): boolean {
  const page = new DOMParser().parseFromString(fetched.text, "text/html");
  const target = range(fetched.depth);
  if (!target) return false;
  const script = page.querySelector("script[type=rz]");
  script?.remove();
  const block = script ? (JSON.parse(script.textContent!) as unknown[]) : undefined;
  let applied = false;
  const apply = () => {
    if (applied) return;
    applied = true;
    const shift = offset(rz.n());
    for (const root of [page.body, ...[...page.querySelectorAll<HTMLTemplateElement>("template[data-rz-e]")].map((t) => t.content)]) {
      for (const element of root.querySelectorAll("[data-rz]")) element.setAttribute("data-rz", shift(element.getAttribute("data-rz")!));
      const walker = page.createTreeWalker(root, NodeFilter.SHOW_COMMENT);
      while (walker.nextNode()) {
        const comment = walker.currentNode as Comment;
        if (/^[0-9a-z]+\.[0-9a-z]+$/.test(comment.data)) comment.data = shift(comment.data);
      }
    }
    if (page.title) document.title = page.title;
    const next = [...page.body.childNodes];
    if (page.querySelector("template[data-rz-s]")) void import("./stream").then((stream) => stream.flush());
    morph(target[0], target[1], next, target[2]);
    if (block) rz.add(block);
    after(range(fetched.depth)?.[1] ?? []);
  };
  if (dispatchEvent(new CustomEvent("rz:swap", { cancelable: true, detail: { kind, apply } }))) apply();
  return true;
}

/** Moves `aria-current="page"` and `data-active` to the anchors that point at the current path. */
function mark(): void {
  for (const anchor of document.querySelectorAll("a[href^='/']")) {
    if (anchor.hasAttribute("aria-current") && !anchor.hasAttribute("data-active")) continue;
    const href = anchor.getAttribute("href")!;
    const active = href[1] !== "/" && href.split(/[?#]/)[0] === current.pathname;
    anchor.toggleAttribute("data-active", active);
    if (active) anchor.setAttribute("aria-current", "page");
    else anchor.removeAttribute("aria-current");
  }
}

function focus(root: Node[]): void {
  const find = (selector: string) =>
    root
      .map((node) =>
        node.nodeType === 1 ? ((node as Element).matches(selector) ? (node as Element) : (node as Element).querySelector(selector)) : null,
      )
      .find(Boolean);
  const target = (find("h1") ?? find("main")) as HTMLElement | null;
  if (!target) return;
  if (!target.hasAttribute("tabindex")) {
    target.setAttribute("tabindex", "-1");
    target.addEventListener("blur", () => target.removeAttribute("tabindex"), { once: true });
  }
  target.focus();
}

/** Warms `href` for a navigation on intent, for 30 seconds or until the next mutation. */
export function prefetch(href: string): void {
  const url = new URL(href, location.href);
  const depth = changed(current, url);
  const key = `${url.href} ${depth}`;
  if (depth < 0 || (prefetched.get(key)?.[0] ?? 0) > Date.now()) return;
  const promise = request(url, depth, { headers: { purpose: "prefetch" } });
  promise.catch(() => prefetched.delete(key));
  prefetched.set(key, [Date.now() + 30_000, promise]);
}

/** Navigates to `href`: fetches the segments that change and morphs them into the page. */
export async function navigate(href: string, pop = false): Promise<void> {
  const url = new URL(href, location.href);
  if (!pop) history.replaceState({ ...history.state, scroll: [scrollX, scrollY] }, "");
  const depth = changed(current, url);
  const moved = url.pathname !== current.pathname;
  const finish = (to: string, root: Node[]) => {
    current = new URL(to);
    mark();
    if (!pop) history.pushState({}, "", to);
    const saved = (history.state as { scroll?: [number, number] } | null)?.scroll;
    if (pop && saved) scrollTo(...saved);
    else if (current.hash) document.getElementById(decodeURIComponent(current.hash.slice(1)))?.scrollIntoView();
    else if (moved) scrollTo(0, 0);
    if (moved) focus(root);
  };
  if (depth < 0) return finish(url.href, []);
  try {
    let fetched = await load(url, depth);
    if (!swap(fetched, "navigation", (root) => finish(fetched.url, root))) {
      fetched = await request(url, 0);
      swap(fetched, "navigation", (root) => finish(fetched.url, root));
    }
  } catch {
    location.assign(url.href);
  }
}

/** Renders the page at the current URL in the browser with `handler`, which then answers every navigation: a client-only application. */
export async function mount(handler: (request: Request) => Promise<Response>): Promise<void> {
  send = async (request) => {
    const response = await handler(request);
    const location = response.headers.get("location");
    if (!location || response.status < 300 || response.status > 399) return response;
    const next = await send(new Request(new URL(location, request.url)));
    return Object.defineProperties(next, { redirected: { value: true }, url: { value: next.url || new URL(location, request.url).href } });
  };
  swap(await request(current, 0), "navigation", () => {});
}

/** Posts a server-tier form through `fetch` and morphs the page it answers with. */
export async function submit(form: HTMLFormElement, submitter: HTMLElement | null): Promise<void> {
  prefetched.clear();
  const action = new URL(submitter?.hasAttribute("formaction") ? (submitter as HTMLButtonElement).formAction : form.action);
  const data = new FormData(form, submitter);
  const body = form.enctype === "multipart/form-data" ? data : new URLSearchParams(data as unknown as Record<string, string>);
  const fetched = await request(action, 0, { method: "POST", body, cache: "no-cache" });
  const url = new URL(fetched.url);
  swap(fetched, "mutation", () => {
    if (!fetched.redirected || url.pathname === current.pathname) return;
    current = url;
    mark();
    history.pushState({}, "", url.href);
    scrollTo(0, 0);
  });
}

/** Posts `input` to mutation `id` as JSON, morphs the page it answers with, and returns the result. */
export async function call(id: string, input: unknown): Promise<unknown> {
  prefetched.clear();
  const target = new URL(current);
  target.searchParams.set("_m", id);
  target.searchParams.set("_b", build);
  const response = await send(
    new Request(target, { method: "POST", body: JSON.stringify(input ?? {}), headers: { "content-type": "application/json" } }),
  );
  if (response.status === 409) {
    location.reload();
    return new Promise(() => {});
  }
  const body = (await response.json()) as {
    result?: unknown;
    status?: number;
    segment?: number;
    html?: string;
    issues?: Issue[];
    message?: string;
  };
  if (body.issues) throw new MutationInputError(body.issues);
  if (!response.ok) throw new Error(body.message);
  swap({ status: body.status!, depth: body.segment!, url: current.href, redirected: false, text: body.html! }, "mutation", () => {});
  return body.result;
}

/** Replaces the content of the nearest `<Errored>` around `node` with its fallback, or reports `error` when none is there. */
export function fault(node: Node, error: unknown): void {
  const isEnd = (sibling: Node) => (sibling as Element).matches?.("template[data-rz-e]");
  const isStart = (sibling: Node) => sibling.nodeType === 8 && (sibling as Comment).data === "e";
  for (let at: Node | null = node; at; at = at.parentNode) {
    let open = 0;
    for (let end = at.nextSibling; end; end = end.nextSibling) {
      if (isStart(end)) open++;
      else if (isEnd(end) && !open--) {
        let start = end.previousSibling!;
        for (let depth = 0; !isStart(start) || depth--; start = start.previousSibling!) if (isEnd(start)) depth++;
        while (start.nextSibling !== end) start.nextSibling!.remove();
        end.replaceWith((end as HTMLTemplateElement).content);
        start.remove();
        return;
      }
    }
  }
  reportError(error);
}
