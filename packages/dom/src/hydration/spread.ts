import { getOwner } from "@rezejs/signals";
import type { ReactiveNode } from "@rezejs/signals/internal/scope";
import { renderEffect } from "@rezejs/signals/render";

import { DelegatedEvents, Properties } from "../../../../crates/reze_compiler/src/html-data.json";
import { assignProp, spread } from "../spread";
import { prepareInsert, queueAttr, queueBool, queueProp, queueStyle, stageRef } from "./native";
import type { Site } from "./protocol";
import { sessionFor, type HydrationSession } from "./session";
import type { StagingBatch, StyleValue } from "./staging";

type Props = Record<string, unknown>;

function prepareProperty(session: HydrationSession, owner: ReactiveNode | undefined, node: Element, site: Site, name: string, value: unknown, previous: unknown, isSvg: boolean): unknown {
  if (name === "style") return queueStyle(node, site, value as StyleValue, previous);
  if (name === "class") queueProp(node, site, name, value);
  else if (name.startsWith("on")) {
    const native = name[2] === ":";
    const lower = native ? name.slice(3) : name.slice(2).toLowerCase();
    const type = !native && lower === "doubleclick" ? "dblclick" : lower;
    session.staging.listener(node, type, value, !native && Object.hasOwn(DelegatedEvents, type), undefined, owner);
  } else if (name.startsWith("prop:")) queueProp(node, site, name.slice(5), value);
  else if (name.startsWith("attr:")) queueAttr(node, site, name.slice(5), value);
  else if (name.startsWith("bool:")) queueBool(node, site, name.slice(5), value);
  else if (!isSvg && Object.hasOwn(Properties, name)) queueProp(node, site, name, value);
  else queueAttr(node, site, name, value);
  return value;
}

export function queueSpread(node: Element, site: Site, props: Props = {}, isSvg = false, hasChildren = false): void {
  const session = sessionFor(node);
  if (session === undefined) {
    spread(node, props, isSvg, hasChildren);
    return;
  }
  const owner = getOwner();
  if (!hasChildren) prepareInsert(node, site, -1, () => props.children);
  renderEffect(() => {
    const ref = props.ref;
    if (typeof ref === "function") stageRef(site, () => { ref(node); });
  });
  const applied: Props = {};
  const batches = new Map<string, StagingBatch>();
  session.adoptBinding(() => batches.clear());
  const apply = (name: string, value: unknown, preparing: boolean): void => {
    if (!preparing) {
      if (value !== applied[name]) applied[name] = assignProp(node, name, value, applied[name], isSvg);
      return;
    }
    const previous = batches.get(name);
    if (previous !== undefined && value === applied[name]) {
      session.staging.retainBatch(previous);
      return;
    }
    const batch = session.staging.beginBatch();
    applied[name] = prepareProperty(session, owner, node, site, name, value, applied[name], isSvg);
    session.staging.endBatch(batch);
    batches.set(name, batch);
  };
  renderEffect(() => {
    const preparing = session.preparing;
    for (const name in props) {
      if (name !== "children" && name !== "ref") apply(name, props[name], preparing);
    }
    for (const name in applied) {
      if (applied[name] !== undefined && !(name in props)) apply(name, undefined, preparing);
    }
  });
}
