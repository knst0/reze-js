import { computed, onCleanup, root, signal, untrack, type Getter, type Setter } from "@rezejs/signals";
import { renderEffect } from "@rezejs/signals/render";

import { insertExpression } from "../insert";
import { ClaimedText, TextRun } from "./claim";
import type { Instance } from "./execution";
import { managedPlans, type NodePlan, type ParentPlan, type RangeController, type RangePlan } from "./plan";
import { HydrationError, type RangeKind, type Site } from "./protocol";
import type { RawGroup } from "./raw";
import type { HydrationSession } from "./session";

export type RangeValue = () => Node[] | RangePlan;

export class BoundRange implements RangeController {
  readonly view: RangeValue;
  private readonly version: Getter<number>;
  private readonly setVersion: Setter<number>;
  private readonly nodes: Node[] = [];
  private readonly current: Node[] = [];
  private revision = 0;
  private active = false;
  private disposed = false;
  private start: Comment | undefined;
  private end: Comment | undefined;
  private raw: RawGroup | undefined;
  private plan: RangePlan | undefined;

  constructor(
    private readonly session: HydrationSession,
    readonly instance: Instance,
    kind: RangeKind,
    site?: Site,
    private readonly place?: (range: BoundRange) => void,
  ) {
    [this.version, this.setVersion] = signal(0);
    this.plan = { kind: "range", token: instance.id, range: kind, instance, ...(site === undefined ? {} : { site }), children: [], controller: this };
    this.view = () => {
      this.track();
      if (!this.active) return this.planned;
      if (this.raw !== undefined) this.raw.upgrade();
      return this.nodes;
    };
    managedPlans.set(this.view, this.plan);
    managedPlans.set(this.plan, this.plan);
    session.adoptBinding(() => this.activate());
    onCleanup(() => {
      this.disposed = true;
      managedPlans.delete(this.view);
      if (this.plan !== undefined) managedPlans.delete(this.plan);
      this.plan = undefined;
      this.raw = undefined;
    });
  }

  get planned(): RangePlan {
    if (this.plan === undefined) throw new HydrationError("range is no longer preparing");
    return this.plan;
  }

  get boundaries(): { start: Comment; end: Comment } {
    if (this.start === undefined || this.end === undefined) throw new HydrationError("range has no physical boundaries");
    return { start: this.start, end: this.end };
  }

  bind(value: unknown): void {
    const source = typeof value === "function" && !managedPlans.has(value) ? computed(() => resolveSource(value)) : value;
    renderEffect(() => {
      if (this.disposed) return;
      if (!this.active) {
        this.session.instances.run(this.instance, () => {
          const plan = this.planned;
          this.session.claims.clear(plan);
          appendPlanned(this.session, plan, source);
          this.place?.(this);
        });
      } else if (this.raw !== undefined) {
        this.raw.update(this.planned, source);
      } else {
        let resolved = source;
        while (typeof resolved === "function") resolved = resolved();
        const start = this.start!;
        const end = this.end!;
        const parent = end.parentNode;
        if (parent === null || start.parentNode !== parent) throw new HydrationError("owned range boundaries were removed");
        this.current.length = 0;
        for (let node = start.nextSibling; node !== null && node !== end; node = node.nextSibling) this.current.push(node);
        insertExpression(parent, resolved, this.current, end, true);
        this.place?.(this);
        this.refresh(true);
      }
    });
  }

  track(): void {
    this.version();
  }

  useRaw(group: RawGroup): void {
    this.raw = group;
  }

  adopt(start: Comment, end: Comment): void {
    this.start = start;
    this.end = end;
    this.raw = undefined;
    this.active = true;
    managedPlans.delete(this.view);
    if (this.plan !== undefined) managedPlans.delete(this.plan);
    this.plan = undefined;
    this.refresh(true);
  }

  private activate(): void {
    if (this.disposed || this.active) return;
    if (this.raw !== undefined) {
      this.active = true;
      this.raw.activate();
      return;
    }
    const plan = this.planned;
    if (plan.claimed !== undefined) this.adopt(plan.claimed.start, plan.claimed.end);
    else {
      const fragment = this.session.element.ownerDocument.createDocumentFragment();
      materializePlan(plan, fragment);
    }
  }

  private refresh(notify: boolean): void {
    let index = 0;
    let changed = false;
    for (let node: Node | null = this.start!; node !== null; node = node.nextSibling) {
      if (this.nodes[index] !== node) changed = true;
      this.nodes[index++] = node;
      if (node === this.end) break;
    }
    if (this.nodes.length !== index) changed = true;
    this.nodes.length = index;
    if (changed && notify) this.setVersion(++this.revision);
  }
}

function resolveSource(value: unknown): unknown {
  while (typeof value === "function" && !managedPlans.has(value)) value = value();
  return value;
}

export function appendPlanned(session: HydrationSession, parent: ParentPlan, value: unknown): void {
  while (value !== null && (typeof value === "function" || typeof value === "object")) {
    const range = managedPlans.get(value);
    if (range !== undefined) {
      range.controller?.track();
      session.claims.attach(parent, range);
      return;
    }
    if (typeof value !== "function") break;
    value = value();
  }
  if (Array.isArray(value)) {
    for (const item of value) appendPlanned(session, parent, item);
  } else if (value != null && typeof value !== "boolean") {
    if (typeof value === "object") {
      const plan = session.claims.handles.get(value as Element);
      if (plan === undefined) throw new HydrationError("unclaimed DOM node during preparation");
      session.claims.attach(parent, plan);
    } else if (typeof value === "string" || typeof value === "number" || typeof value === "bigint") {
      session.claims.attach(parent, session.claims.text(String(value)));
    } else {
      throw new HydrationError("unsupported insertion during preparation");
    }
  }
}

export function managedRange(session: HydrationSession, kind: RangeKind, role: string, site: Site, build: () => unknown, place?: (range: BoundRange) => void): RangeValue {
  const instance = session.instances.reserve(role, site);
  session.instances.own(instance);
  return session.instances.run(instance, () => {
    const range = new BoundRange(session, instance, kind, site, place);
    range.bind(untrack(build));
    return range.view;
  });
}

export function mountRange(session: HydrationSession, build: () => unknown): void {
  session.instances.run(session.instances.root, () => root(() => {
    const range = new BoundRange(session, session.instances.root, "fragment");
    session.mount(range.planned);
    range.bind(untrack(build));
  }));
}

function materializePlan(plan: NodePlan, parent: Node): void {
  const document = parent.ownerDocument!;
  if (plan.kind === "text") {
    const node = document.createTextNode(plan.handle.data);
    plan.handle.bind(new ClaimedText(new TextRun(node, parent, null, [plan.handle.data]), 0));
    parent.appendChild(node);
  } else if (plan.kind === "range") {
    const start = document.createComment("");
    const end = document.createComment("");
    parent.appendChild(start);
    for (const child of plan.children) materializePlan(child, parent);
    parent.appendChild(end);
    plan.controller?.adopt(start, end);
  } else {
    if (plan.kind === "element" && !plan.opaque) {
      const container = plan.ns === "" && plan.tag === "template" ? (plan.node as HTMLTemplateElement).content : plan.node;
      for (const child of plan.children) materializePlan(child, container);
    }
    parent.appendChild(plan.node);
  }
}
