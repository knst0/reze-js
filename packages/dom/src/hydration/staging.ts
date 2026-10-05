import { getOwner, onCleanup, runWithOwner, untrack } from "@rezejs/signals";
import type { ReactiveNode } from "@rezejs/signals/internal/scope";

import { setAttribute, setAttributeNS, setBoolAttribute } from "../attributes";
import { className, type ClassValue } from "../class-name";
import { addEventListener, delegateEvents } from "../events";
import { style } from "../style";
import { DirtyForms } from "./forms";
import type { ElementPlan } from "./plan";
import type { HydrationReplay } from "./replay";

interface OwnerGate {
  readonly owner: ReactiveNode;
  live: boolean;
}

interface PendingWrite {
  gate: OwnerGate;
  readonly node: Element;
  readonly kind: "attribute" | "namespace" | "boolean" | "property" | "class";
  readonly name: string;
  readonly value: unknown;
  readonly namespace?: string;
  readonly classes?: unknown;
}

interface PendingEffect {
  gate: OwnerGate;
  readonly run: () => void | (() => void);
  readonly cleanupOwner?: ReactiveNode;
}

interface ClassElement extends Element {
  $$class?: unknown;
}

export type StyleValue = string | { readonly [property: string]: string | number | null | undefined } | null | undefined;

export interface StagingBatch {
  readonly writeStart: number;
  readonly effectStart: number;
  writeEnd: number;
  effectEnd: number;
}

export class CommitStaging {
  private readonly gates = new WeakMap<ReactiveNode, OwnerGate>();
  private readonly writes: PendingWrite[] = [];
  private readonly effects: PendingEffect[] = [];
  private readonly mirrors = new Map<Element, HTMLElement>();

  constructor(readonly execution: HydrationReplay) {}

  attribute(node: Element, name: string, value: unknown, namespace?: string): void {
    this.writes.push({
      gate: this.gate(),
      node,
      name,
      value: value == null || value === false ? null : String(value),
      kind: namespace === undefined ? "attribute" : "namespace",
      ...(namespace === undefined ? {} : { namespace }),
    });
  }

  boolean(node: Element, name: string, value: unknown): void {
    this.writes.push({ gate: this.gate(), node, name, value: !!value, kind: "boolean" });
  }

  property(node: Element, name: string, value: unknown): void {
    this.writes.push({ gate: this.gate(), node, name, value, kind: "property" });
  }

  classes(plan: ElementPlan, value: ClassValue, initial?: readonly (readonly [string, string | null])[]): void {
    const mirror = this.mirror(plan.node, initial);
    className(mirror, value);
    this.writes.push({
      gate: this.gate(),
      node: plan.node,
      name: "class",
      value: mirror.getAttribute("class"),
      kind: "class",
      classes: (mirror as ClassElement).$$class,
    });
  }

  toggle(
    plan: ElementPlan,
    token: string,
    value: unknown,
    previous: unknown,
    initial?: readonly (readonly [string, string | null])[],
  ): boolean {
    const next = !!value;
    if (next !== previous) {
      const mirror = this.mirror(plan.node, initial);
      mirror.classList.toggle(token, next);
      this.writes.push({
        gate: this.gate(),
        node: plan.node,
        name: "class",
        value: mirror.getAttribute("class"),
        kind: "class",
        classes: (mirror as ClassElement).$$class,
      });
    }
    return next;
  }

  styles(plan: ElementPlan, value: StyleValue, previous: unknown, initial?: readonly (readonly [string, string | null])[]): unknown {
    const mirror = this.mirror(plan.node, initial);
    const result = style(mirror, value, previous);
    this.attribute(plan.node, "style", mirror.getAttribute("style"));
    return result;
  }

  beginBatch(): StagingBatch {
    return {
      writeStart: this.writes.length,
      effectStart: this.effects.length,
      writeEnd: this.writes.length,
      effectEnd: this.effects.length,
    };
  }

  endBatch(batch: StagingBatch): void {
    batch.writeEnd = this.writes.length;
    batch.effectEnd = this.effects.length;
  }

  retainBatch(batch: StagingBatch): void {
    const gate = this.gate();
    for (let i = batch.writeStart; i < batch.writeEnd; i++) this.writes[i]!.gate = gate;
    for (let i = batch.effectStart; i < batch.effectEnd; i++) this.effects[i]!.gate = gate;
  }

  defer(run: () => void | (() => void), cleanupOwner?: ReactiveNode): void {
    this.effects.push({ gate: this.gate(), run, cleanupOwner });
  }

  listener(node: Element, name: string, handler: unknown, delegated: boolean, data?: unknown, cleanupOwner?: ReactiveNode): void {
    const pair = Array.isArray(handler) ? [handler[0], handler[1]] : handler;
    this.defer(() => installListener(node, name, pair, delegated, data), cleanupOwner);
  }

  commit(forms: DirtyForms): void {
    for (const write of this.writes) {
      if (!write.gate.live || !forms.allows(write.node, write.name, write.value)) continue;
      runWithOwner(write.gate.owner, () => {
        switch (write.kind) {
          case "attribute":
            setAttribute(write.node, write.name, write.value);
            break;
          case "namespace":
            setAttributeNS(write.node, write.namespace!, write.name, write.value);
            break;
          case "boolean":
            setBoolAttribute(write.node, write.name, write.value);
            break;
          case "class":
            setAttribute(write.node, "class", write.value);
            (write.node as ClassElement).$$class = write.classes;
            break;
          case "property": {
            const node = write.node as unknown as Record<string, unknown>;
            if (node[write.name] !== write.value) node[write.name] = write.value;
            break;
          }
        }
      });
    }
    this.writes.length = 0;
  }

  commitEffects(): void {
    for (const effect of this.effects) {
      if (!effect.gate.live) continue;
      runWithOwner(effect.cleanupOwner ?? effect.gate.owner, () => {
        const cleanup = untrack(effect.run);
        if (cleanup !== undefined) onCleanup(cleanup);
      });
    }
    this.effects.length = 0;
  }

  release(): void {
    this.writes.length = 0;
    this.effects.length = 0;
    this.mirrors.clear();
  }

  private gate(): OwnerGate {
    const owner = getOwner();
    if (owner === undefined) return this.execution.run(() => this.gate());
    let gate = this.gates.get(owner);
    if (gate === undefined || !gate.live) {
      gate = { owner, live: true };
      this.gates.set(owner, gate);
      const current = gate;
      onCleanup(() => {
        current.live = false;
      });
    }
    return gate;
  }

  private mirror(node: Element, initial: readonly (readonly [string, string | null])[] | undefined): HTMLElement {
    let mirror = this.mirrors.get(node);
    if (mirror === undefined) {
      mirror = node.ownerDocument.createElement("div");
      for (const [name, value] of initial ?? []) {
        if (name === "class" || name === "style") mirror.setAttribute(name, value ?? "");
      }
      this.mirrors.set(node, mirror);
    }
    return mirror;
  }
}

export function installListener(node: Element, name: string, handler: unknown, delegated: boolean, data?: unknown): () => void {
  if (delegated) {
    const target = node as unknown as Record<string, unknown>;
    const key = `$$${name}`;
    const dataKey = `${key}Data`;
    const previous = target[key];
    const previousData = target[dataKey];
    const listener = Array.isArray(handler) ? handler[0] : handler;
    target[key] = listener;
    target[dataKey] = Array.isArray(handler) ? handler[1] : data;
    delegateEvents([name], node.ownerDocument);
    return () => {
      if (target[key] === listener) {
        target[key] = previous;
        target[dataKey] = previousData;
      }
    };
  }
  addEventListener(node, name, handler);
  const listener = Array.isArray(handler) ? handler[0] : handler;
  const options = Array.isArray(handler) ? handler[1] : undefined;
  return () => {
    if (listener) node.removeEventListener(name, listener as EventListener, options as EventListenerOptions | boolean | undefined);
  };
}
