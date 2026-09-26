/** `null`, `undefined` and `false` remove the attribute. */
export function setAttribute(node: Element, name: string, value: unknown): void {
  if (value == null || value === false) {
    node.removeAttribute(name);
  } else {
    node.setAttribute(name, value as string);
  }
}

/** `name` is qualified (`xlink:href`); `null`, `undefined` and `false` remove the attribute. */
export function setAttributeNS(node: Element, ns: string, name: string, value: unknown): void {
  if (value == null || value === false) {
    node.removeAttributeNS(ns, name.slice(name.indexOf(":") + 1));
  } else {
    node.setAttributeNS(ns, name, value as string);
  }
}

export function setBoolAttribute(node: Element, name: string, value: unknown): void {
  node.toggleAttribute(name, !!value);
}
