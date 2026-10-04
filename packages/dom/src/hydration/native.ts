import { onCleanup, untrack } from "@rezejs/signals";
import { renderEffect } from "@rezejs/signals/render";

import { setAttribute, setAttributeNS, setBoolAttribute } from "../attributes";
import { className, type ClassValue } from "../class-name";
import { delegateEvents } from "../events";
import { insert } from "../insert";
import { style } from "../style";
import { moduleExecution } from "./execution";
import { TextHandle, type ElementPlan, type NodePlan, type PlanHandle } from "./plan";
import { HydrationError, type NamespaceKey, type Site } from "./protocol";
import { BoundRange } from "./range";
import { HydrationSession, preparingSession, sessionFor } from "./session";
import { installListener, type StyleValue } from "./staging";

export function isPreparing(): boolean {
  return preparingSession() !== undefined;
}

export function claimRoot(site: Site, tag?: string, namespace: NamespaceKey = ""): Element {
  const session = preparingSession();
  if (session === undefined) throw new HydrationError("native claim outside hydration preparation", site);
  const instance = session.instances.reserve("e", site);
  session.instances.own(instance);
  const plan = session.claims.createNative(instance, site, tag, namespace);
  for (const child of session.claims.native.get(plan.node)!.paths.values()) {
    if (child.kind === "text") session.attach(child.handle);
    else if (child.kind === "element" || child.kind === "marker") session.attach(child.node);
  }
  return plan.node;
}

function claim(root: Element, path: string, site: Site): NodePlan {
  const plan = sessionFor(root)?.claims.native.get(root)?.paths.get(path);
  if (plan === undefined) throw new HydrationError(`missing native claim path ${path}`, site);
  return plan;
}

export function claimElement(root: Element, path: string, site: Site): Element {
  const plan = claim(root, path, site);
  if (plan.kind !== "element") throw new HydrationError(`expected element at ${path}`, site);
  return plan.node;
}

export function claimText(root: Element, path: string, site: Site): TextHandle {
  const plan = claim(root, path, site);
  if (plan.kind !== "text") throw new HydrationError(`expected text at ${path}`, site);
  return plan.handle;
}

export function claimMarker(root: Element, path: string, site: Site): Comment {
  const plan = claim(root, path, site);
  if (plan.kind !== "marker") throw new HydrationError(`expected marker at ${path}`, site);
  return plan.node;
}

function elementPlan(session: HydrationSession, node: Element, site: Site): ElementPlan {
  const plan = session.claims.handles.get(node);
  if (plan?.kind !== "element") throw new HydrationError("unclaimed native element", site);
  return plan;
}

function initialAttributes(plan: ElementPlan, site: Site): readonly (readonly [string, string | null])[] | undefined {
  return site.layout !== undefined && "nodes" in site.layout
    ? site.layout.nodes[plan.index ?? 0]?.attrs : undefined;
}

function domString(value: unknown): string {
  if (typeof value === "symbol") throw new TypeError("Cannot convert a Symbol value to a string");
  return String(value);
}

export function queueText(node: TextHandle | Text, _site: Site, value: unknown): void {
  node.data = value === null ? "" : domString(value);
}

export function queueAttr(node: Element, _site: Site, name: string, value: unknown): void {
  const session = sessionFor(node);
  if (session === undefined) setAttribute(node, name, value);
  else session.staging.attribute(node, name, value);
}

export function queueAttrNS(node: Element, _site: Site, namespace: string, name: string, value: unknown): void {
  const session = sessionFor(node);
  if (session === undefined) setAttributeNS(node, namespace, name, value);
  else session.staging.attribute(node, name, value, namespace);
}

export function queueBool(node: Element, _site: Site, name: string, value: unknown): void {
  const session = sessionFor(node);
  if (session === undefined) setBoolAttribute(node, name, value);
  else session.staging.boolean(node, name, value);
}

export function queueProp(node: Element, site: Site, name: string, value: unknown): void {
  const session = sessionFor(node);
  if (session === undefined) {
    if (name === "class") className(node, value as ClassValue);
    else (node as unknown as Record<string, unknown>)[name] = value;
    return;
  }
  const plan = elementPlan(session, node, site);
  if (name === "class") {
    session.staging.classes(plan, value as ClassValue, initialAttributes(plan, site));
    return;
  }
  if (name === "textContent") {
    session.claims.clear(plan);
    plan.opaque = false;
    value = value == null ? "" : domString(value);
    if (value !== "") session.claims.attach(plan, session.claims.text(value as string));
  } else if (name === "innerHTML") {
    session.claims.clear(plan);
    plan.opaque = true;
    value = value === null ? "" : domString(value);
  }
  session.staging.property(node, name, value);
}

export function queueToggle(node: Element, site: Site, token: string, value: unknown, previous?: unknown): boolean {
  const session = sessionFor(node);
  if (session === undefined) {
    const next = !!value;
    if (next !== previous) node.classList.toggle(token, next);
    return next;
  }
  const plan = elementPlan(session, node, site);
  return session.staging.toggle(plan, token, value, previous, initialAttributes(plan, site));
}

export function queueStyle(node: Element, site: Site, value: StyleValue, previous?: unknown): unknown {
  const session = sessionFor(node);
  if (session === undefined) return style(node as HTMLElement, value, previous);
  const plan = elementPlan(session, node, site);
  return session.staging.styles(plan, value, previous, initialAttributes(plan, site));
}

export function stageListener(node: Element, _site: Site, name: string, handler: unknown, delegated: boolean, data?: unknown): void {
  const session = sessionFor(node);
  if (session !== undefined) session.staging.listener(node, name, handler, delegated, data);
  else onCleanup(installListener(node, name, handler, delegated, data));
}

export function stageRef(_site: Site, callback: () => void): void {
  const session = preparingSession();
  if (session === undefined) untrack(callback);
  else session.deferCommit(() => { callback(); });
}

export function deref<T extends Element>(node: T): T {
  return node;
}

export function prepareEffect(callback: () => void, _site: Site): void {
  renderEffect(callback);
}

export function stageDelegation(moduleId: string, events: string[]): void {
  const execution = moduleExecution(moduleId);
  if (execution instanceof HydrationSession && execution.preparing) {
    execution.deferCommit(() => delegateEvents(events, execution.element.ownerDocument));
  } else delegateEvents(events);
}

function prepareInsertion(parent: Element, site: Site, slot: number, value: unknown, anchor: PlanHandle | undefined, append: boolean): void {
  const session = sessionFor(parent);
  if (session === undefined) {
    if (append) insert(parent, value, null);
    else if (anchor === undefined) insert(parent, value);
    else insert(parent, value, anchor as Node);
    return;
  }
  let target: ElementPlan | undefined = elementPlan(session, parent, site);
  let before = anchor === undefined ? undefined : session.claims.handles.get(anchor);
  const instance = session.instances.reserve(slot < 0 ? "spread" : `i${slot.toString(36)}`, site, target.instance);
  session.instances.own(instance);
  session.instances.run(instance, () => {
    const range = new BoundRange(session, instance, "insertion", site, binding => {
      if (target !== undefined) {
        if (!append && before === undefined) session.claims.clear(target);
        target.opaque = false;
        session.claims.attach(target, binding.planned, before);
      }
    });
    session.adoptBinding(() => { target = undefined; before = undefined; });
    range.bind(value);
  });
}

export function prepareInsert(parent: Element, site: Site, slot: number, value: unknown, anchor?: PlanHandle): void {
  prepareInsertion(parent, site, slot, value, anchor, false);
}

export function prepareAppend(parent: Element, site: Site, slot: number, value: unknown): void {
  prepareInsertion(parent, site, slot, value, undefined, true);
}
