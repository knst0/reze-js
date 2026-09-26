type StyleProperties = { readonly [property: string]: string | number | null | undefined };

/**
 * `value` is `cssText` or kebab-case properties, where `null` and `undefined` remove a property; `null` or `undefined`
 * as `value` removes the `style` attribute. Pass the previous return value as `prev` so only changed properties are
 * written.
 */
export function style(node: Element & ElementCSSInlineStyle, value: string | StyleProperties | null | undefined, prev?: unknown): unknown {
  const css = node.style;
  if (value == null) {
    node.removeAttribute("style");
    return value;
  }
  if (typeof value === "string") {
    css.cssText = value;
    return value;
  }
  const last = typeof prev === "object" && prev !== null ? (prev as StyleProperties) : undefined;
  if (last === undefined) {
    css.cssText = "";
  } else {
    for (const property in last) {
      if (value[property] == null) {
        css.removeProperty(property);
      }
    }
  }
  for (const property in value) {
    const next = value[property];
    if (next != null && (last === undefined || next !== last[property])) {
      css.setProperty(property, next as string);
    }
  }
  return value;
}
