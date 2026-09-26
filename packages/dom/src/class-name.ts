type ClassToggles = { readonly [classes: string]: unknown };

export type ClassValue = string | number | boolean | null | undefined | ClassToggles | readonly ClassValue[];

type ClassTokens = Record<string, true>;

interface ClassNode extends Element {
  $$class?: string | ClassTokens;
}

const Whitespace = /\s/;
const WhitespaceRun = /\s+/;

/**
 * Applies `value` as the element's class list. A string replaces the `class` attribute; `null`, `undefined` and `false`
 * remove it. An object turns on every space-separated class of each truthy key; an array (nesting allowed) merges its
 * items in order, where a later object key overrides an earlier one and strings and numbers (`0` included) are keys.
 * Objects and arrays are diffed against the tokens applied by the previous call on this element.
 */
export function className(node: Element, value: ClassValue): void {
  const el = node as ClassNode;
  const last = el.$$class;
  if (value == null || value === false) {
    node.removeAttribute("class");
    el.$$class = undefined;
    return;
  }
  if (typeof value === "string") {
    if (value !== last) {
      node.setAttribute("class", value);
    }
    el.$$class = value;
    return;
  }
  const next = classTokens(value);
  const list = node.classList;
  let applied: ClassTokens | undefined;
  if (typeof last === "string") {
    node.removeAttribute("class");
  } else if (last !== undefined) {
    applied = last;
    for (const token in applied) {
      if (next[token] === undefined) {
        list.remove(token);
      }
    }
  }
  for (const token in next) {
    if (applied === undefined || applied[token] === undefined) {
      list.add(token);
    }
  }
  el.$$class = next;
}

function classTokens(value: ClassValue): ClassTokens {
  const tokens: ClassTokens = {};
  if (Array.isArray(value)) {
    const merged: Record<string, unknown> = {};
    mergeClassArray(value, merged);
    addTruthyKeys(merged, tokens);
  } else if (typeof value === "object" && value !== null) {
    addTruthyKeys(value as ClassToggles, tokens);
  }
  return tokens;
}

function mergeClassArray(list: readonly ClassValue[], merged: Record<string, unknown>): void {
  for (let i = 0; i < list.length; i++) {
    const item = list[i];
    if (Array.isArray(item)) {
      mergeClassArray(item, merged);
    } else if (typeof item === "object" && item !== null) {
      Object.assign(merged, item);
    } else if (item || item === 0) {
      merged[item as string] = true;
    }
  }
}

function addTruthyKeys(toggles: ClassToggles, tokens: ClassTokens): void {
  for (const classes in toggles) {
    if (!toggles[classes]) {
      continue;
    }
    if (!Whitespace.test(classes)) {
      if (classes) {
        tokens[classes] = true;
      }
      continue;
    }
    for (const token of classes.split(WhitespaceRun)) {
      if (token) {
        tokens[token] = true;
      }
    }
  }
}
