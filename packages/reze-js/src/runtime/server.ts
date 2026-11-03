type Thunk = () => string;

class Markup {
  constructor(readonly content: string | Thunk) {}
}

interface HeadEntry {
  depth: number;
  html: string;
}

export interface RenderContext {
  depth: number;
  params: Record<string, string | undefined>;
  head(key: string | null, html: string): string;
  id(): string;
  /** Adds the state entry of a client component instance and returns its id. */
  scope(component: number, events: string, captures: unknown[], eager: boolean): string;
  /** Adds the state entry of a callback call that client code captures values of. */
  row(parent: string, kind: number, captures: unknown[]): string;
}

type Component = (props: Record<string, unknown>, $r: RenderContext) => string;

interface RouteModule {
  default: Component;
}

type Segment = { kind: "static"; name: string } | { kind: "param" | "optional" | "splat"; name: string };

export interface Route {
  pattern: string;
  segments: Segment[];
  chain: RouteModule[];
  /** Route node of the client chunk, when the page has client code. */
  chunk: number | null;
}

/** URLs of the client code. */
export interface Assets {
  dispatcher: string;
  chunk(node: number): string;
}

const text = /[&<]/g;
const attribute = /[&"]/g;
const entities: Record<string, string> = { "&": "&amp;", "<": "&lt;", '"': "&quot;" };
const replace = (c: string) => entities[c]!;

export const $h = (html: string) => new Markup(html);
export const $l = (render: Thunk) => new Markup(render);

export function $t(value: unknown): string {
  if (value == null || typeof value === "boolean") return "";
  if (value instanceof Markup) {
    return typeof value.content === "string" ? value.content : value.content();
  }
  return String(value as string).replace(text, replace);
}

export function $a(name: string, value: unknown): string {
  if (typeof value === "boolean" && name.startsWith("aria-")) return ` ${name}="${value}"`;
  if (value == null || value === false) return "";
  if (value === true) return ` ${name}`;
  return ` ${name}="${String(value as string).replace(attribute, replace)}"`;
}

export function $s(props: Record<string, unknown> | null | undefined): string {
  let out = "";
  for (const name in props) {
    if (name === "children" || name === "ref" || /^on[A-Z]/.test(name)) continue;
    out += $a(name, props[name]);
  }
  return out;
}

export function $show<T>(value: T, then: (value: NonNullable<T>) => string, otherwise?: Thunk): string {
  if (value) return then(value as NonNullable<T>);
  return otherwise ? otherwise() : "";
}

export function $for<T>(each: readonly T[] | null | undefined, row: (item: T, index: number) => string, fallback?: Thunk): string {
  if (!each?.length) return fallback ? fallback() : "";
  let out = "";
  for (let index = 0; index < each.length; index++) out += row(each[index]!, index);
  return out;
}

export function $repeat(count: number, row: (index: number) => string, fallback?: Thunk): string {
  if (!(count > 0)) return fallback ? fallback() : "";
  let out = "";
  for (let index = 0; index < count; index++) out += row(index);
  return out;
}

function match(route: Route, parts: string[]): Record<string, string | undefined> | null {
  const params: Record<string, string | undefined> = {};
  let index = 0;
  for (const segment of route.segments) {
    const part = parts[index];
    switch (segment.kind) {
      case "static":
        if (part?.toLowerCase() !== segment.name) return null;
        index++;
        break;
      case "param":
        if (part === undefined) return null;
        params[segment.name] = part;
        index++;
        break;
      case "optional":
        if (part !== undefined) {
          params[segment.name] = part;
          index++;
        }
        break;
      case "splat":
        if (index >= parts.length) return null;
        params[segment.name] = parts.slice(index).join("/");
        index = parts.length;
        break;
    }
  }
  return index === parts.length ? params : null;
}

function render(route: Route, params: Record<string, string | undefined>, assets?: Assets): string {
  const keyed = new Map<string, HeadEntry>([
    ["meta:charset", { depth: -1, html: '<meta charset="utf-8">' }],
    ["meta:name=viewport", { depth: -1, html: '<meta name="viewport" content="width=device-width, initial-scale=1">' }],
  ]);
  const unkeyed: string[] = [];
  let ids = 0;
  const entries: unknown[][] = [];
  const events = new Set<string>();
  let eager = false;
  const $r: RenderContext = {
    depth: 0,
    params,
    head(key, html) {
      if (key === null) {
        if (!unkeyed.includes(html)) unkeyed.push(html);
      } else if ((keyed.get(key)?.depth ?? -2) <= this.depth) {
        keyed.set(key, { depth: this.depth, html });
      }
      return "";
    },
    id: () => `rz-${ids++}`,
    scope(component, names, captures, isEager) {
      for (const name of names.split(" ")) if (name) events.add(name);
      eager ||= isEager;
      return (entries.push([component, ...captures]) - 1).toString(36);
    },
    row(parent, kind, captures) {
      return (entries.push([~parseInt(parent, 36), kind, ...captures]) - 1).toString(36);
    },
  };
  const level = (depth: number): string => {
    const children =
      depth + 1 < route.chain.length
        ? $l(() => {
            const previous = $r.depth;
            $r.depth = depth + 1;
            try {
              return level(depth + 1);
            } finally {
              $r.depth = previous;
            }
          })
        : undefined;
    return route.chain[depth]!.default({ params, children }, $r);
  };
  const root = level(0);
  let head = "";
  for (const entry of keyed.values()) head += entry.html;
  head += unkeyed.join("");
  let state = "";
  if (entries.length > 0 && assets && route.chunk !== null) {
    head += `<script type="module" async src="${assets.dispatcher.replace(attribute, replace)}"></script>`;
    const block = [[...events].join(" "), assets.chunk(route.chunk), eager ? 1 : 0, ...entries];
    state = `<script type="rz">${JSON.stringify(block).replaceAll("<", "\\u003c")}</script>`;
  }
  return `<!doctype html><html><head>${head}</head><body>${root}${state}</body></html>`;
}

export interface HandlerOptions {
  onRequest?(request: Request): void;
  onError?(error: unknown, request: Request): void;
}

export interface AppOptions extends HandlerOptions {
  assets?: Assets;
}

export type Handler = (request: Request) => Promise<Response>;

export function createApp(routes: Route[], options: AppOptions = {}): Handler {
  return async (request) => {
    options.onRequest?.(request);
    const parts = new URL(request.url).pathname.split("/").filter(Boolean).map(decodeURIComponent);
    for (const route of routes) {
      const params = match(route, parts);
      if (!params) continue;
      try {
        return new Response(render(route, params, options.assets), {
          headers: { "content-type": "text/html; charset=utf-8" },
        });
      } catch (error) {
        options.onError?.(error, request);
        return new Response("Internal Server Error", { status: 500 });
      }
    }
    return new Response("Not Found", { status: 404 });
  };
}
