import { root, untrack } from "@rezejs/signals";

import { IslandClose, IslandOpen, Properties, classTokens, createComponent } from "./dom";
import { nextComponentScope, nextHydrationKey, withKeyScope } from "./hydration";
import type { JSX } from "./jsx";

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
    return withKeyScope("", () =>
      root((dispose) => {
        try {
          return ssrChild(code());
        } finally {
          dispose();
        }
      }),
    );
  } finally {
    isStaticArea = wasStaticArea;
  }
}

/**
 * `createComponent` for an island boundary. In the static area it renders the island between
 * `<!--$id:scope:props-->` and `<!--/$-->`, `scope` being the key scope it opens and `props`
 * JSON, and the island itself renders in the client area. Throws `[ISLAND_PROPS]` when `props`
 * is not JSON: `null`, booleans, strings, finite numbers other than `-0`, dense arrays and
 * plain objects, without cycles.
 */
export function ssrIsland<P>(id: string, Comp: (props: P) => JSX.Element, props: P): JSX.Element {
  if (!isStaticArea) return createComponent(Comp, props);
  const json = islandJSON(id, props, "props", []);
  const scope = nextComponentScope()!;
  isStaticArea = false;
  try {
    const html = withKeyScope(scope, () => ssrChild(untrack(() => Comp(props))));
    return new RenderedHTML(
      `<!--${IslandOpen}${id}:${scope}:${json}-->${html}<!--${IslandClose}-->`,
    ) as unknown as JSX.Element;
  } finally {
    isStaticArea = true;
  }
}

function islandJSON(id: string, value: unknown, path: string, ancestors: object[]): string {
  if (value === null) return "null";
  switch (typeof value) {
    case "boolean":
      return String(value);
    case "string":
      return JSON.stringify(value).replace(/[<>-]/g, (c) => IslandJSONEscapes[c]!);
    case "number":
      if (!Number.isFinite(value) || Object.is(value, -0)) {
        throw islandPropsError(id, path, Object.is(value, -0) ? "-0" : String(value));
      }
      return String(value);
    case "object": {
      if (ancestors.includes(value)) throw islandPropsError(id, path, "a cycle");
      ancestors.push(value);
      let json: string;
      if (Array.isArray(value)) {
        json = "[";
        for (let i = 0; i < value.length; i++) {
          if (!(i in value)) throw islandPropsError(id, path, "a sparse array");
          json += (i ? "," : "") + islandJSON(id, value[i], `${path}[${i}]`, ancestors);
        }
        json += "]";
      } else {
        const proto = Object.getPrototypeOf(value);
        if (proto !== Object.prototype && proto !== null) {
          throw islandPropsError(id, path, `a ${proto?.constructor?.name ?? "non-plain"} object`);
        }
        json = "{";
        for (const key of Reflect.ownKeys(value)) {
          const keyPath = `${path}.${String(key)}`;
          if (typeof key === "symbol" || !Object.prototype.propertyIsEnumerable.call(value, key)) {
            throw islandPropsError(id, keyPath, "a symbol or non-enumerable key");
          }
          json +=
            (json.length > 1 ? "," : "") +
            islandJSON(id, key, keyPath, ancestors) +
            ":" +
            islandJSON(id, (value as Record<string, unknown>)[key], keyPath, ancestors);
        }
        json += "}";
      }
      ancestors.pop();
      return json;
    }
    default:
      throw islandPropsError(id, path, value === undefined ? "undefined" : `a ${typeof value}`);
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
