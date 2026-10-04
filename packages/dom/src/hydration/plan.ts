import { ClaimedText, ClaimIndex, type ClaimedRange, type TextRun } from "./claim";
import type { Instance } from "./execution";
import type { DirtyForms } from "./forms";
import { HydrationError, type LayoutNode, type NamespaceKey, type RangeKind, type Site } from "./protocol";
import { RawGroup } from "./raw";

export class TextHandle {
  claimed: ClaimedText | undefined;

  constructor(private value: string) {}

  get data(): string {
    return this.value;
  }

  set data(value: string) {
    this.value = value;
    if (this.claimed !== undefined) this.claimed.data = value;
  }

  bind(claimed: ClaimedText): void {
    this.claimed = claimed;
  }
}

interface PlanBase {
  parent?: ParentPlan;
  readonly index?: number;
}

export interface ElementPlan extends PlanBase {
  readonly kind: "element";
  readonly node: Element;
  readonly tag: string;
  readonly ns: NamespaceKey;
  readonly token?: string;
  readonly site?: Site;
  readonly instance: Instance;
  children: NodePlan[];
  opaque: boolean;
}

export interface TextPlan extends PlanBase {
  readonly kind: "text";
  readonly handle: TextHandle;
}

export interface MarkerPlan extends PlanBase {
  readonly kind: "marker";
  readonly node: Comment;
}

export interface ForeignPlan extends PlanBase {
  readonly kind: "foreign";
  readonly node: Node;
}

export interface RangeController {
  track(): void;
  adopt(start: Comment, end: Comment): void;
  useRaw(group: RawGroup): void;
}

export interface RangePlan extends PlanBase {
  readonly kind: "range";
  readonly token: string;
  readonly site?: Site;
  readonly range: RangeKind;
  readonly instance: Instance;
  children: NodePlan[];
  claimed?: ClaimedRange;
  controller?: RangeController;
}

export type ParentPlan = ElementPlan | RangePlan;
export type NodePlan = ParentPlan | TextPlan | MarkerPlan | ForeignPlan;
export type PlanHandle = Element | Comment | TextHandle;
export const managedPlans = new WeakMap<object, RangePlan>();

export interface NativePlan {
  readonly root: ElementPlan;
  readonly paths: ReadonlyMap<string, NodePlan>;
}

const Namespaces = {
  "": "http://www.w3.org/1999/xhtml",
  svg: "http://www.w3.org/2000/svg",
  math: "http://www.w3.org/1998/Math/MathML",
};
const RawText = new Set(["textarea", "title", "style", "script", "xmp", "iframe", "noembed", "noframes"]);
const ParserContainers = new Set(["tbody", "tr", "colgroup"]);

export class ClaimPlan {
  readonly handles = new Map<PlanHandle, NodePlan>();
  readonly native = new Map<Element, NativePlan>();
  readonly texts: TextHandle[] = [];
  private readonly consumed = new Set<LayoutNode>();

  constructor(readonly index: ClaimIndex) {}

  createNative(instance: Instance, site: Site, tag?: string, ns: NamespaceKey = ""): ElementPlan {
    const layout = site.layout;
    const statics = layout !== undefined && "nodes" in layout ? layout.nodes : undefined;
    const claimed = this.index.roots.get(instance.id);
    if (claimed !== undefined && claimed.layout.site !== site.key) throw new HydrationError(`native root site mismatch ${instance.id}`, site);
    const nodes: NodePlan[] = [];
    const paths = new Map<string, NodePlan>();
    const count = statics?.length ?? 1;
    for (let index = 0; index < count; index += 1) {
      const descriptor = statics?.[index];
      const existing = claimed?.statics.get(index);
      let plan: NodePlan;
      if (descriptor?.kind === "text") {
        const handle = new TextHandle(descriptor.text ?? "");
        plan = { kind: "text", handle, index };
        this.texts.push(handle);
        this.handles.set(handle, plan);
      } else if (descriptor?.kind === "marker") {
        const node = existing !== undefined && !(existing instanceof ClaimedText) && existing.nodeType === 8
          ? existing as Comment : this.index.root.ownerDocument.createComment("");
        plan = { kind: "marker", node, index };
        this.handles.set(node, plan);
      } else {
        const name = descriptor?.tag ?? tag;
        const namespace = descriptor?.ns ?? ns;
        if (name === undefined) throw new HydrationError("native claim has no element layout", site);
        const node = existing !== undefined && !(existing instanceof ClaimedText) && existing.nodeType === 1
          ? existing as Element : this.index.root.ownerDocument.createElementNS(Namespaces[namespace], name);
        if (existing === undefined) {
          for (const [key, value] of descriptor?.attrs ?? []) node.setAttribute(key, value ?? "");
        }
        plan = { kind: "element", node, tag: name, ns: namespace, instance, index,
          ...(index === 0 ? { token: instance.id, site } : {}), children: [], opaque: false };
        this.handles.set(node, plan);
      }
      nodes.push(plan);
    }
    for (let index = 0; index < nodes.length; index += 1) {
      const descriptor = statics?.[index];
      const plan = nodes[index]!;
      if (plan.kind !== "element") continue;
      for (const child of descriptor?.children ?? []) this.attach(plan, nodes[child]!);
    }
    const root = nodes[0] as ElementPlan;
    const visit = (node: NodePlan, path: string): void => {
      paths.set(path, node);
      if (node.kind !== "element") return;
      const prefix = node.tag === "template" && node.ns === "" ? (path === "" ? "c" : `${path}.c`) : path;
      for (let index = 0; index < node.children.length; index += 1) {
        visit(node.children[index]!, prefix === "" ? String(index) : `${prefix}.${index}`);
      }
    };
    visit(root, "");
    this.native.set(root.node, { root, paths });
    return root;
  }

  attach(parent: ParentPlan, node: NodePlan, before?: NodePlan): void {
    for (let ancestor: ParentPlan | undefined = parent; ancestor !== undefined; ancestor = ancestor.parent) {
      if (ancestor === node) throw new HydrationError("cyclic hydration insertion");
    }
    if (node.parent !== undefined) {
      const siblings = node.parent.children;
      const index = siblings.indexOf(node);
      if (index !== -1) siblings.splice(index, 1);
    }
    node.parent = parent;
    const index = before === undefined ? -1 : parent.children.indexOf(before);
    if (index === -1) parent.children.push(node);
    else parent.children.splice(index, 0, node);
  }

  clear(parent: ParentPlan): void {
    for (const child of parent.children) child.parent = undefined;
    parent.children.length = 0;
  }

  text(value: string): TextPlan {
    const handle = new TextHandle(value);
    this.texts.push(handle);
    const plan: TextPlan = { kind: "text", handle };
    this.handles.set(handle, plan);
    return plan;
  }

  validate(roots: readonly NodePlan[]): void {
    this.consumed.clear();
    this.match(roots, this.index.layout);
    if (this.consumed.size !== this.index.nodes.size) throw new HydrationError("unconsumed live DOM layout");
    this.index.validate();
  }

  commitText(forms?: DirtyForms): void {
    const runs = new Set<TextRun>();
    for (const text of this.texts) {
      const claimed = text.claimed;
      if (claimed === undefined) continue;
      claimed.run.parts[claimed.index] = text.data;
      runs.add(claimed.run);
    }
    for (const run of runs) {
      if (run.parent.nodeType === 1 && forms !== undefined && !forms.allows(run.parent as Element, "value", undefined)) continue;
      run.commit();
    }
  }

  release(): void {
    this.handles.clear();
    this.native.clear();
    this.texts.length = 0;
    this.consumed.clear();
  }

  private match(plans: readonly NodePlan[], layout: readonly LayoutNode[]): void {
    let index = 0;
    for (const expected of layout) {
      const plan = plans[index];
      if (expected.kind === "element" && expected.index === undefined && expected.token === undefined
        && expected.ns === "" && ParserContainers.has(expected.tag)
        && (plan?.kind !== "element" || plan.tag !== expected.tag)) {
        const count = this.projectedLength(expected.children);
        this.consumed.add(expected);
        this.match(plans.slice(index, index + count), expected.children);
        index += count;
        continue;
      }
      if (plan === undefined || plan.kind !== expected.kind) {
        const token = expected.kind === "range" || expected.kind === "element" ? expected.token : undefined;
        throw new HydrationError(`final ${expected.kind} topology mismatch${token === undefined ? "" : ` at ${token}`}: found ${plan?.kind ?? "no node"}`);
      }
      this.consumed.add(expected);
      if (plan.kind === "text" && expected.kind === "text") {
        const claimed = this.index.nodes.get(expected);
        if (!(claimed instanceof ClaimedText)) throw new HydrationError("missing claimed text");
        plan.handle.bind(claimed);
      } else if (plan.kind === "marker") {
        if (this.index.nodes.get(expected) !== plan.node) throw new HydrationError("static marker mismatch");
      } else if (plan.kind === "range" && expected.kind === "range") {
        if (plan.token !== expected.token || plan.range !== expected.range || plan.site?.key !== expected.site
          || plan.instance.id !== expected.ownerId || plan.instance.retired) {
          throw new HydrationError(`range mismatch ${expected.token}`, plan.site);
        }
        plan.claimed = this.index.ranges.get(expected.token)!;
        this.match(plan.children, expected.children);
      } else if (plan.kind === "element" && expected.kind === "element") {
        if (plan.tag !== expected.tag || plan.ns !== expected.ns || plan.token !== expected.token
          || plan.site?.key !== expected.site || plan.node !== this.index.nodes.get(expected)
          || plan.opaque !== (expected.opaque === true) || plan.instance.retired) {
          throw new HydrationError(`native topology mismatch ${expected.token ?? expected.tag}`, plan.site);
        }
        if (!plan.opaque) {
          if (plan.ns === "" && RawText.has(plan.tag)) this.rawText(plan, expected.children);
          else this.match(plan.children, expected.children);
        }
      }
      index += 1;
    }
    if (index !== plans.length) throw new HydrationError("extra live nodes in final hydration tree");
  }

  private projectedLength(layout: readonly LayoutNode[]): number {
    let count = 0;
    for (const node of layout) {
      count += node.kind === "element" && node.index === undefined && node.token === undefined
        && node.ns === "" && ParserContainers.has(node.tag) ? this.projectedLength(node.children) : 1;
    }
    return count;
  }

  private rawText(plan: ElementPlan, layout: readonly LayoutNode[]): void {
    if (layout.length !== 1 || layout[0]!.kind !== "text") throw new HydrationError("invalid raw text layout", plan.site);
    const descriptor = layout[0]!;
    const claimed = this.index.nodes.get(descriptor);
    if (!(claimed instanceof ClaimedText)) throw new HydrationError("missing raw text run", plan.site);
    new RawGroup(plan, claimed.run).bind();
    this.consumed.add(descriptor);
  }
}
