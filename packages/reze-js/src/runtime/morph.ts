type Field = HTMLInputElement & HTMLOptionElement;

/** Whether attribute `name` of the live `element` holds state the user owns, which a morph keeps. */
function owned(element: Element, name: string): boolean {
  const field = element as Field;
  switch (name) {
    case "open":
      return element.tagName === "DETAILS" || element.tagName === "DIALOG";
    case "value":
      return (element.tagName === "INPUT" || element.tagName === "TEXTAREA") && field.value !== field.defaultValue;
    case "checked":
      return element.tagName === "INPUT" && field.checked !== field.defaultChecked;
    case "selected":
      return element.tagName === "OPTION" && field.selected !== field.defaultSelected;
  }
  return false;
}

const same = (live: Node, next: Node) =>
  live.nodeType === next.nodeType &&
  live.nodeName !== "TEMPLATE" &&
  (live.nodeType !== 1 || ((live as Element).tagName === (next as Element).tagName && (live as Element).id === (next as Element).id));

function update(live: Node, next: Node): void {
  if (live.nodeType !== 1) {
    if (live.nodeValue !== next.nodeValue) live.nodeValue = next.nodeValue;
    return;
  }
  const element = live as Element;
  for (const { name } of Array.from(element.attributes)) {
    if (!owned(element, name) && !(next as Element).hasAttribute(name)) element.removeAttribute(name);
  }
  for (const { name, value } of (next as Element).attributes) {
    if (!owned(element, name) && element.getAttribute(name) !== value) element.setAttribute(name, value);
  }
  if (!owned(element, "value")) morph(element, [...element.childNodes], [...next.childNodes], null);
}

/**
 * Morphs the live nodes `nodes` of `parent`, which end before `end`, into `next`: nodes that match
 * by type, tag, and `id` are updated in place, and the rest are inserted or removed.
 */
export function morph(parent: Node, nodes: Node[], next: Node[], end: Node | null): void {
  let index = 0;
  for (const node of next) {
    const live = nodes[index];
    if (live && same(live, node)) {
      update(live, node);
      index++;
      continue;
    }
    const id = node.nodeType === 1 ? (node as Element).id : "";
    const found = id ? nodes.findIndex((candidate, at) => at > index && same(candidate, node)) : -1;
    if (found > 0) {
      while (index < found) parent.removeChild(nodes[index++]!);
      update(nodes[index++]!, node);
    } else {
      parent.insertBefore(node, live ?? end);
    }
  }
  while (index < nodes.length) parent.removeChild(nodes[index++]!);
}
