import { ClaimedText, TextRun } from "./claim";
import { managedPlans, TextHandle, type ElementPlan, type NodePlan, type ParentPlan, type RangePlan } from "./plan";
import { HydrationError } from "./protocol";

export class RawGroup {
  private upgraded = false;

  constructor(readonly root: ElementPlan, readonly run: TextRun) {}

  bind(): void {
    this.run.parts.length = 0;
    this.bindNodes(this.root.children);
  }

  activate(): void {
    this.root.parent = undefined;
  }

  update(range: RangePlan, value: unknown): void {
    while (typeof value === "function" && !managedPlans.has(value)) value = value();
    const type = typeof value;
    if (range.children.length === 1 && range.children[0]!.kind === "text"
      && (value == null || type === "boolean" || type === "string" || type === "number" || type === "bigint")) {
      range.children[0]!.handle.data = value == null || type === "boolean" ? "" : String(value);
      return;
    }
    if (value !== null && type === "object" && !Array.isArray(value) && !managedPlans.has(value as object) && (value as Node).nodeType === undefined) return;
    if (type === "symbol") return;
    const children: NodePlan[] = [];
    this.collect(value, children, false);
    for (const child of range.children) child.parent = undefined;
    range.children = children;
    for (const child of children) child.parent = range;
    if (this.hasForeign(this.root.children)) this.upgrade();
    else {
      this.bind();
      this.run.commit();
    }
  }

  upgrade(): void {
    if (this.upgraded) return;
    this.upgraded = true;
    const parent = this.root.node;
    const document = parent.ownerDocument;
    const fragment = document.createDocumentFragment();
    let previous = this.run.node;
    const adopted: { range: RangePlan; start: Comment; end: Comment }[] = [];
    const append = (plan: NodePlan, target: Node): void => {
      if (plan.kind === "text") {
        const node = previous ?? document.createTextNode(plan.handle.data);
        previous = undefined;
        node.data = plan.handle.data;
        target.appendChild(node);
        plan.handle.bind(new ClaimedText(new TextRun(node, parent, null, [plan.handle.data]), 0));
      } else if (plan.kind === "range") {
        const start = document.createComment(`rz:1:${plan.token}:start`);
        const end = document.createComment(`rz:1:${plan.token}:end`);
        target.appendChild(start);
        for (const child of plan.children) append(child, target);
        target.appendChild(end);
        adopted.push({ range: plan, start, end });
      } else {
        target.appendChild(plan.node);
      }
    };
    for (const child of this.root.children) append(child, fragment);
    parent.replaceChildren(fragment);
    for (const { range, start, end } of adopted) range.controller?.adopt(start, end);
    this.root.children.length = 0;
    this.run.parts.length = 0;
  }

  private bindNodes(nodes: readonly NodePlan[]): void {
    for (const node of nodes) {
      if (node.kind === "text") {
        const index = this.run.parts.length;
        this.run.parts.push(node.handle.data);
        const claimed = node.handle.claimed;
        if (claimed?.run === this.run) claimed.index = index;
        else node.handle.bind(new ClaimedText(this.run, index));
      } else if (node.kind === "range") {
        node.controller?.useRaw(this);
        this.bindNodes(node.children);
      } else if (node.kind !== "marker") {
        throw new HydrationError("element inside raw text run", this.root.site);
      }
    }
  }

  private collect(value: unknown, output: NodePlan[], inArray: boolean): void {
    while (value !== null && (typeof value === "function" || typeof value === "object")) {
      const plan = managedPlans.get(value);
      if (plan !== undefined) {
        plan.controller?.track();
        this.detach(plan);
        output.push(plan);
        return;
      }
      if (typeof value !== "function") break;
      value = value();
    }
    if (value == null || typeof value === "boolean") return;
    if (Array.isArray(value)) {
      for (const item of value) this.collect(item, output, true);
    } else if ((typeof value === "object" || typeof value === "function") && (value as Node).nodeType !== undefined) {
      output.push({ kind: "foreign", node: value as Node });
    } else if (inArray || typeof value === "string" || typeof value === "number" || typeof value === "bigint") {
      output.push({ kind: "text", handle: new TextHandle(String(value)) });
    }
  }

  private detach(node: NodePlan): void {
    const parent: ParentPlan | undefined = node.parent;
    if (parent === undefined) return;
    const index = parent.children.indexOf(node);
    if (index !== -1) parent.children.splice(index, 1);
    node.parent = undefined;
  }

  private hasForeign(nodes: readonly NodePlan[]): boolean {
    for (const node of nodes) {
      if (node.kind === "foreign" || node.kind === "element") return true;
      if (node.kind === "range" && this.hasForeign(node.children)) return true;
    }
    return false;
  }
}
