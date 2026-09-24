import { flushSync, root, untrack } from "@rezejs/signals";

import {
  IslandClose,
  IslandOpen,
  Properties,
  SlotClose,
  SlotOpen,
  classTokens,
  createComponent,
  mergeProps,
} from "./dom";
import {
  nextComponentScope,
  nextHydrationKey,
  resumeKeyScope,
  withKeyScope,
  withServerRender,
} from "./hydration";
import type { JSX } from "./jsx";
import {
  StreamClose,
  StreamOpen,
  withActiveStream,
  type BoundaryStream,
  type StreamBoundary,
} from "./stream";

// oxlint-disable-next-line typescript/no-explicit-any
type Any = any;
type Props = Record<string, Any>;

/** HTML produced by a server template: rendered as is, never escaped again. */
class RenderedHTML {
  constructor(readonly html: string) {}
}

/** Whether `ssrIsland` marks islands: in the static area of `renderToString(code, true)`. */
let isStaticArea = false;

/**
 * Renders `code()` to HTML for `hydrate` to adopt. Runs the components once under a root that
 * is disposed right after: effects never run, and async components render what they have.
 * With `islands`, rendering starts in the static area, where `ssrIsland` marks islands for
 * `hydrateIslands`.
 */
export function renderToString(code: () => JSX.Element, islands?: boolean): string {
  const wasStaticArea = isStaticArea;
  isStaticArea = islands === true;
  try {
    return withServerRender(() =>
      withKeyScope("", () =>
        root((dispose) => {
          try {
            return ssrChild(code());
          } finally {
            dispose();
          }
        }),
      ),
    );
  } finally {
    isStaticArea = wasStaticArea;
  }
}

/**
 * `createComponent` for an island boundary. In the static area it renders the island between
 * `<!--$id:scope:props:mode-->` and `<!--/$-->`, `scope` being the key scope it opens, `props`
 * JSON and `mode` the `island:load` mode (`eager` by default). Each slot renders first, in the
 * parent scope, wrapped as `<!--$slot:name-->…<!--/$slot-->`; the island itself renders in the
 * client area, reading its props and its slot HTML. Throws `[ISLAND_PROPS]` when `props` is
 * not JSON: `null`, booleans, strings, finite numbers other than `-0`, dense arrays and plain
 * objects, without cycles.
 */
export function ssrIsland<P>(
  id: string,
  Comp: (props: P) => JSX.Element,
  props: P,
  slots?: Props | null,
  mode?: string,
): JSX.Element {
  if (!isStaticArea) return createComponent(Comp, (slots ? mergeProps(props, slots) : props) as P);
  const rendered = slots ? renderSlots(slots) : undefined;
  const json = islandJSON(props, "props", [], (path, found) => islandPropsError(id, path, found));
  const scope = nextComponentScope()!;
  isStaticArea = false;
  try {
    const html = withKeyScope(scope, () =>
      ssrChild(untrack(() => Comp({ ...(props as object), ...rendered } as P))),
    );
    return new RenderedHTML(
      `<!--${IslandOpen}${id}:${scope}:${json}:${mode ?? "eager"}-->${html}<!--${IslandClose}-->`,
    ) as unknown as JSX.Element;
  } finally {
    isStaticArea = true;
  }
}

/** Each slot as HTML in the parent scope, wrapped in its `<!--$slot:name-->` range. */
function renderSlots(slots: Props): Record<string, RenderedHTML> {
  const rendered: Record<string, RenderedHTML> = {};
  for (const key of Object.keys(slots)) {
    const html = ssrChild(slots[key]);
    rendered[key] = new RenderedHTML(
      `<!--${SlotOpen}${encodeURIComponent(key)}-->${html}<!--${SlotClose}-->`,
    );
  }
  return rendered;
}

function islandJSON(
  value: unknown,
  path: string,
  ancestors: object[],
  fail: (path: string, found: string) => Error,
): string {
  if (value === null) return "null";
  switch (typeof value) {
    case "boolean":
      return String(value);
    case "string":
      return JSON.stringify(value).replace(/[<>-]/g, (c) => IslandJSONEscapes[c]!);
    case "number":
      if (!Number.isFinite(value) || Object.is(value, -0)) {
        throw fail(path, Object.is(value, -0) ? "-0" : String(value));
      }
      return String(value);
    case "object": {
      if (ancestors.includes(value)) throw fail(path, "a cycle");
      ancestors.push(value);
      let json: string;
      if (Array.isArray(value)) {
        json = "[";
        for (let i = 0; i < value.length; i++) {
          if (!(i in value)) throw fail(path, "a sparse array");
          json += (i ? "," : "") + islandJSON(value[i], `${path}[${i}]`, ancestors, fail);
        }
        json += "]";
      } else {
        const proto = Object.getPrototypeOf(value);
        if (proto !== Object.prototype && proto !== null) {
          throw fail(path, `a ${proto?.constructor?.name ?? "non-plain"} object`);
        }
        json = "{";
        for (const key of Reflect.ownKeys(value)) {
          const keyPath = `${path}.${String(key)}`;
          if (typeof key === "symbol" || !Object.prototype.propertyIsEnumerable.call(value, key)) {
            throw fail(keyPath, "a symbol or non-enumerable key");
          }
          json +=
            (json.length > 1 ? "," : "") +
            islandJSON(key, keyPath, ancestors, fail) +
            ":" +
            islandJSON((value as Record<string, unknown>)[key], keyPath, ancestors, fail);
        }
        json += "}";
      }
      ancestors.pop();
      return json;
    }
    default:
      throw fail(path, value === undefined ? "undefined" : `a ${typeof value}`);
  }
}

function islandPropsError(id: string, path: string, found: string): Error {
  return new Error(`[ISLAND_PROPS] island "${id}": ${path} is ${found}, which is not JSON`);
}

const IslandJSONEscapes: Record<string, string> = {
  "<": "\\u003c",
  ">": "\\u003e",
  "-": "\\u002d",
};

/** A server template: `strings` interleaved with the rendered dynamic `parts`. */
export function ssr(strings: readonly string[], ...parts: string[]): JSX.Element {
  let html = strings[0]!;
  for (let i = 0; i < parts.length; i++) html += parts[i]! + strings[i + 1]!;
  return new RenderedHTML(html) as unknown as JSX.Element;
}

/** ` data-hk="…"` for a template root while `renderToString` runs. */
export function ssrHydrationKey(): string {
  const key = nextHydrationKey();
  return key === undefined ? "" : ` data-hk="${key}"`;
}

/** A child value as HTML, the way `insert` would show it: text escaped, functions read. */
export function ssrChild(value: Any): string {
  while (typeof value === "function") value = value();
  if (value == null || typeof value === "boolean") return "";
  if (value instanceof RenderedHTML) return value.html;
  if (Array.isArray(value)) {
    let html = "";
    for (const item of value) html += ssrChild(item);
    return html;
  }
  return String(value).replace(/[&<]/g, (c) => (c === "&" ? "&amp;" : "&lt;"));
}

/** `innerHTML`: the value as HTML, unescaped. */
export function ssrRaw(value: Any): string {
  return value == null ? "" : String(value);
}

/** ` name="value"`; `null`, `undefined` and `false` render nothing, as `setAttribute` removes. */
export function ssrAttribute(name: string, value: Any): string {
  if (value == null || value === false) return "";
  const escaped = String(value).replace(/[&"]/g, (c) => (c === "&" ? "&amp;" : "&quot;"));
  return ` ${name}="${escaped}"`;
}

export function ssrBoolAttribute(name: string, value: unknown): string {
  return value ? " " + name : "";
}

/** `class` as `className` applies it: a string as is, anything else as its true tokens. */
export function ssrClass(value: unknown): string {
  if (value == null || value === false) return "";
  if (typeof value === "string") return ssrAttribute("class", value);
  const tokens = Object.keys(classTokens(value)).filter((k) => k && k !== "undefined");
  return tokens.length ? ssrAttribute("class", tokens.join(" ")) : "";
}

/** `style` from a string or a property object; `null` and `undefined` values are skipped. */
export function ssrStyle(value: unknown): string {
  if (value == null) return "";
  if (typeof value === "string") return ssrAttribute("style", value);
  let css = "";
  for (const key in value as Props) {
    const v = (value as Props)[key];
    if (v != null) css += `${css ? ";" : ""}${key}:${v}`;
  }
  return css ? ssrAttribute("style", css) : "";
}

/**
 * The attributes `spread` would set: events, refs, `prop:` keys and the `textContent` and
 * `innerHTML` properties are client-only; `children` is rendered by the caller.
 */
export function ssrSpread(props: Props, isSVG?: boolean): string {
  let html = "";
  for (const name in props) {
    if (name === "children" || name === "ref" || name.startsWith("on")) continue;
    const value = props[name];
    if (name === "style") html += ssrStyle(value);
    else if (name === "class") html += ssrClass(value);
    else if (name.startsWith("attr:")) html += ssrAttribute(name.slice(5), value);
    else if (name.startsWith("bool:")) html += ssrBoolAttribute(name.slice(5), value);
    else if (name.startsWith("prop:")) continue;
    else if (!isSVG && Object.hasOwn(Properties, name)) {
      if (name === "value") html += ssrAttribute(name, value);
      else if (name === "checked" || name === "selected") html += ssrBoolAttribute(name, value);
    } else html += ssrAttribute(name, value);
  }
  return html;
}

/**
 * Streams `code()` as HTML for `hydrate`. The shell comes at once, rendered like
 * `renderToString`; an async component with a pending await is a placeholder
 * `<!--$s:<id>-->…<!--/$s-->` around its pre-load HTML, `id` being its key scope plus the index
 * of the await. As an await settles, the component renders again with the settled values and
 * `<template data-reze-chunk="<id>" data-reze-values="<json>">…</template>` follows, the value
 * serialized like island props (`[STREAM_VALUES]` errors the stream otherwise; `undefined`
 * omits the attribute). The stream closes once every await settled or after `timeoutMs`;
 * placeholders of pending or rejected awaits stay. Cancelling the stream stops the rendering.
 */
export function renderToStream(
  code: () => JSX.Element,
  options?: { timeoutMs?: number },
): ReadableStream<Uint8Array> {
  let stream!: ServerStream;
  return new ReadableStream<Uint8Array>({
    start(controller) {
      stream = new ServerStream(controller);
      stream.start(code, options?.timeoutMs ?? 30000);
    },
    cancel() {
      stream.stop();
    },
  });
}

/**
 * Await `index` of an async component: registers `load()` with the stream rendering the
 * component, which waits for it and streams the component again once it settled. Outside a
 * stream it is `load()`.
 */
export function ssrAwait<T>(
  boundary: StreamBoundary | undefined,
  index: number,
  load: () => T,
): T | Promise<Awaited<T>> {
  const stream = boundary?.stream;
  return stream instanceof ServerStream ? stream.await(boundary!, index, load()) : load();
}

interface ServerBoundary {
  content: (() => JSX.Element) | undefined;
  /** The index of the pending await, while there is one. */
  awaiting: number | undefined;
}

class ServerStream implements BoundaryStream {
  private readonly encoder = new TextEncoder();
  private readonly boundaries = new Map<StreamBoundary, ServerBoundary>();
  private pending = 0;
  private isClosed = false;
  private cancelTimeout: (() => void) | undefined;
  private dispose: (() => void) | undefined;

  constructor(private readonly controller: ReadableStreamDefaultController<Uint8Array>) {}

  start(code: () => JSX.Element, timeoutMs: number): void {
    let shell: string;
    try {
      shell = this.render(() =>
        withKeyScope("", () =>
          root((dispose) => {
            this.dispose = dispose;
            return ssrChild(code());
          }),
        ),
      );
    } catch (error) {
      this.dispose?.();
      throw error;
    }
    this.controller.enqueue(this.encoder.encode(shell));
    if (!this.pending) {
      this.close();
      return;
    }
    const timeout = setTimeout(() => this.close(), timeoutMs);
    this.cancelTimeout = () => clearTimeout(timeout);
  }

  stop(): void {
    this.isClosed = true;
    this.cancelTimeout?.();
    this.dispose?.();
  }

  output(boundary: StreamBoundary, content: () => JSX.Element): JSX.Element {
    this.state(boundary).content = content;
    return (() => new RenderedHTML(this.boundaryHTML(boundary))) as unknown as JSX.Element;
  }

  await<T>(boundary: StreamBoundary, index: number, value: T): Promise<Awaited<T>> {
    const promise = Promise.resolve(value);
    this.pending++;
    this.state(boundary).awaiting = index;
    promise.then(
      (settled) => queueMicrotask(() => this.resolve(boundary, index, settled)),
      () => queueMicrotask(() => this.reject()),
    );
    return promise;
  }

  /**
   * Runs after `trackAsync` applied `value` (its callbacks were registered after this one's),
   * so flushing runs the component's next await step before its content renders again.
   */
  private resolve(boundary: StreamBoundary, index: number, value: unknown): void {
    if (this.isClosed) return;
    this.pending--;
    this.state(boundary).awaiting = undefined;
    const id = boundary.scope.id + index;
    try {
      const values =
        value === undefined
          ? ""
          : ssrAttribute(
              "data-reze-values",
              islandJSON(
                value,
                "value",
                [],
                (path, found) =>
                  new Error(
                    `[STREAM_VALUES] chunk "${id}": ${path} is ${found}, which is not JSON`,
                  ),
              ),
            );
      const html = this.render(() => {
        flushSync();
        return this.boundaryHTML(boundary);
      });
      this.controller.enqueue(
        this.encoder.encode(`<template data-reze-chunk="${id}"${values}>${html}</template>`),
      );
    } catch (error) {
      this.stop();
      this.controller.error(error);
      return;
    }
    if (!this.pending) this.close();
  }

  private reject(): void {
    if (this.isClosed) return;
    this.pending--;
    if (!this.pending) this.close();
  }

  private close(): void {
    this.stop();
    this.controller.close();
  }

  private state(boundary: StreamBoundary): ServerBoundary {
    let state = this.boundaries.get(boundary);
    if (!state)
      this.boundaries.set(boundary, (state = { content: undefined, awaiting: undefined }));
    return state;
  }

  private boundaryHTML(boundary: StreamBoundary): string {
    const { content, awaiting } = this.state(boundary);
    const html = resumeKeyScope(boundary.scope, () => ssrChild(content));
    if (awaiting === undefined) return html;
    return `<!--${StreamOpen}${boundary.scope.id}${awaiting}-->${html}<!--${StreamClose}-->`;
  }
  private render<T>(fn: () => T): T {
    const wasStaticArea = isStaticArea;
    isStaticArea = false;
    try {
      return withServerRender(() => withActiveStream(this, fn));
    } finally {
      isStaticArea = wasStaticArea;
    }
  }
}
