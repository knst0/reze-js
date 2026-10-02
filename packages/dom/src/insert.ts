import { renderEffect } from "@rezejs/signals/render";

import { reconcileArrays } from "./reconcile";

type Current = Node | Node[] | string | undefined | (() => Current);
type Slot = Node | (() => unknown);

/**
 * Inserts `value` into `parent` before `marker`; a function becomes a binding that re-inserts what it returns.
 * `marker === undefined` means `value` is the only content of `parent`, so text and clearing go through `textContent`;
 * `null` appends at the end.
 */
export function insert(parent: Node, value: unknown, marker?: Node | null): void {
  const initial: Current = marker === undefined ? undefined : [];
  if (typeof value === "function") {
    renderEffect<Current>((current) => insertExpression(parent, (value as () => unknown)(), current, marker), initial);
  } else {
    insertExpression(parent, value, initial, marker);
  }
}

/** {@link insert} after the existing children of `parent`. */
export function append(parent: Node, value: unknown): void {
  insert(parent, value, null);
}

function insertExpression(parent: Node, value: unknown, current: Current, marker: Node | null | undefined, unwrap?: boolean): Current {
  while (typeof current === "function") {
    current = current();
  }
  if (value === current) {
    return current;
  }
  const isRange = marker !== undefined;
  if (isRange) {
    parent = (current as Node[])[0]?.parentNode ?? parent;
  }
  const type = typeof value;
  if (type === "string" || type === "number" || type === "bigint") {
    const text = String(value);
    if (isRange) {
      const nodes = current as Node[];
      const first = nodes[0];
      if (first !== undefined && first.nodeType === 3) {
        if ((first as Text).data !== text) {
          (first as Text).data = text;
        }
        return nodes.length === 1 ? nodes : cleanChildren(parent, nodes, marker, first);
      }
      return cleanChildren(parent, nodes, marker, document.createTextNode(text));
    }
    if (typeof current === "string" && current !== "") {
      (parent.firstChild as Text).data = text;
    } else {
      parent.textContent = text;
    }
    return text;
  }
  if (value == null || type === "boolean") {
    return cleanChildren(parent, current, marker);
  }
  if (type === "function") {
    renderEffect(() => {
      let resolved = (value as () => unknown)();
      while (typeof resolved === "function") {
        resolved = resolved();
      }
      current = insertExpression(parent, resolved, current, marker);
    });
    return () => current;
  }
  if (Array.isArray(value)) {
    const slots: Slot[] = [];
    if (normalizeArray(slots, value, current, unwrap)) {
      renderEffect(() => {
        current = insertExpression(parent, slots, current, marker, true);
      });
      return () => current;
    }
    const nodes = slots as Node[];
    if (nodes.length === 0) {
      const cleared = cleanChildren(parent, current, marker);
      return isRange ? cleared : nodes;
    }
    if (Array.isArray(current)) {
      if (current.length === 0) {
        appendNodes(parent, nodes, marker ?? null);
      } else {
        reconcileArrays(parent, current, nodes);
      }
    } else {
      if (current) {
        parent.textContent = "";
      }
      appendNodes(parent, nodes, null);
    }
    return nodes;
  }
  if ((value as Node).nodeType !== undefined) {
    const node = value as Node;
    if (Array.isArray(current)) {
      if (isRange) {
        return cleanChildren(parent, current, marker, node);
      }
      cleanChildren(parent, current, null, node);
    } else if (parent.firstChild === null) {
      parent.appendChild(node);
    } else {
      parent.replaceChild(node, parent.firstChild);
    }
    return node;
  }
  return current;
}

/**
 * Flattens `array` into `slots`, reusing the text node `current` holds at the same position. Functions are resolved
 * with `unwrap`, otherwise kept as slots; returns whether one was kept, so the caller must bind and unwrap.
 */
function normalizeArray(slots: Slot[], array: readonly unknown[], current: Current, unwrap?: boolean): boolean {
  let hasFunction = false;
  for (let i = 0; i < array.length; i++) {
    if (normalizeItem(slots, array[i], current, unwrap)) {
      hasFunction = true;
    }
  }
  return hasFunction;
}

function normalizeItem(slots: Slot[], item: unknown, current: Current, unwrap?: boolean): boolean {
  if (typeof item === "function") {
    if (!unwrap) {
      slots.push(item as () => unknown);
      return true;
    }
    do {
      item = item();
    } while (typeof item === "function");
  }
  if (item == null || typeof item === "boolean") {
    return false;
  }
  if (Array.isArray(item)) {
    return normalizeArray(slots, item, current, unwrap);
  }
  if ((item as Node).nodeType !== undefined) {
    slots.push(item as Node);
    return false;
  }
  // oxlint-disable-next-line typescript/no-base-to-string -- array items that are not nodes render as their string form
  const text = String(item);
  const reusable = Array.isArray(current) ? current[slots.length] : undefined;
  if (reusable !== undefined && reusable.nodeType === 3) {
    if ((reusable as Text).data !== text) {
      (reusable as Text).data = text;
    }
    slots.push(reusable);
  } else {
    slots.push(document.createTextNode(text));
  }
  return false;
}

function appendNodes(parent: Node, nodes: readonly Node[], marker: Node | null): void {
  if (nodes.length < 2) {
    for (let i = 0; i < nodes.length; i++) {
      parent.insertBefore(nodes[i]!, marker);
    }
    return;
  }
  const fragment = document.createDocumentFragment();
  for (let i = 0; i < nodes.length; i++) {
    fragment.appendChild(nodes[i]!);
  }
  parent.insertBefore(fragment, marker);
}

/**
 * Removes what `current` holds and leaves `replacement`, or an empty text node keeping the position, in its place.
 * Without a marker everything in `parent` is cleared.
 */
function cleanChildren(parent: Node, current: Current, marker: Node | null | undefined, replacement?: Node): Current {
  if (marker === undefined) {
    parent.textContent = "";
    return "";
  }
  const node = replacement ?? document.createTextNode("");
  const nodes = current as Node[];
  if (nodes.length === 0) {
    parent.insertBefore(node, marker);
    return [node];
  }
  let isPlaced = false;
  for (let i = nodes.length - 1; i >= 0; i--) {
    const el = nodes[i]!;
    if (el === node) {
      isPlaced = true;
      continue;
    }
    const isChild = el.parentNode === parent;
    if (!isPlaced && i === 0) {
      if (isChild) {
        parent.replaceChild(node, el);
      } else {
        parent.insertBefore(node, marker);
      }
    } else if (isChild) {
      (el as ChildNode).remove();
    }
  }
  return [node];
}
