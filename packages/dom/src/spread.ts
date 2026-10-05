import { renderEffect } from "@rezejs/signals/render";

import { DelegatedEvents, Properties } from "../../../crates/reze_compiler/src/html-data.json";
import { setAttribute, setBoolAttribute } from "./attributes";
import { className, type ClassValue } from "./class-name";
import { addEventListener, delegateEvents } from "./events";
import { insert } from "./insert";
import { use } from "./ref";
import { style } from "./style";

type Props = Record<string, unknown>;

/**
 * Applies `props` to `node` and re-applies them when what they read changes. `props.children` is inserted unless
 * `hasChildren`; a function `props.ref` receives `node`.
 */
export function spread(node: Element, props: Props = {}, isSVG?: boolean, hasChildren?: boolean): void {
  if (!hasChildren) {
    insert(node, () => props.children);
  }
  renderEffect(() => {
    const ref = props.ref;
    if (typeof ref === "function") {
      use(ref as (el: Element) => void, node);
    }
  });
  const applied: Props = {};
  renderEffect(() => {
    for (const name in props) {
      if (name === "children" || name === "ref") {
        continue;
      }
      const value = props[name];
      if (value !== applied[name]) {
        applied[name] = assignProp(node, name, value, applied[name], isSVG);
      }
    }
    for (const name in applied) {
      if (applied[name] !== undefined && !(name in props)) {
        applied[name] = assignProp(node, name, undefined, applied[name], isSVG);
      }
    }
  });
}

export function assignProp(node: Element, name: string, value: unknown, prev: unknown, isSVG: boolean | undefined): unknown {
  if (name === "style") {
    return style(node as HTMLElement, value as Parameters<typeof style>[1], prev);
  }
  if (name === "class") {
    className(node, value as ClassValue);
  } else if (name.startsWith("on")) {
    setListener(node, name, value, prev);
  } else if (name.startsWith("prop:")) {
    (node as unknown as Props)[name.slice(5)] = value;
  } else if (name.startsWith("attr:")) {
    setAttribute(node, name.slice(5), value);
  } else if (name.startsWith("bool:")) {
    setBoolAttribute(node, name.slice(5), value);
  } else if (!isSVG && Object.hasOwn(Properties, name)) {
    (node as unknown as Props)[name] = value;
  } else {
    setAttribute(node, name, value);
  }
  return value;
}

function setListener(node: Element, name: string, handler: unknown, prev: unknown): void {
  const isNative = name[2] === ":";
  const lowered = isNative ? name.slice(3) : name.slice(2).toLowerCase();
  const type = !isNative && lowered === "doubleclick" ? "dblclick" : lowered;
  if (!isNative && Object.hasOwn(DelegatedEvents, type)) {
    const target = node as unknown as Props;
    const isPair = Array.isArray(handler);
    target["$$" + type] = isPair ? handler[0] : handler;
    target["$$" + type + "Data"] = isPair ? handler[1] : undefined;
    delegateEvents([type]);
    return;
  }
  if (Array.isArray(prev)) {
    node.removeEventListener(type, prev[0] as EventListener, prev[1] as EventListenerOptions | undefined);
  } else if (prev) {
    node.removeEventListener(type, prev as EventListener);
  }
  addEventListener(node, type, handler);
}
