import { root } from "@rezejs/signals";

import { Properties, classTokens } from "./dom";
import { nextHydrationKey, withKeyRoot } from "./hydration";
import type { JSX } from "./jsx";

// oxlint-disable-next-line typescript/no-explicit-any
type Any = any;
type Props = Record<string, Any>;

/** HTML produced by a server template: rendered as is, never escaped again. */
class RenderedHTML {
  constructor(readonly html: string) {}
}

/**
 * Renders `code()` to HTML for `hydrate` to adopt. Runs the components once under a root that
 * is disposed right after: effects never run, and async components render what they have.
 */
export function renderToString(code: () => JSX.Element): string {
  return withKeyRoot(() =>
    root((dispose) => {
      try {
        return ssrChild(code());
      } finally {
        dispose();
      }
    }),
  );
}

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
