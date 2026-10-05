import { HydrationError, type LayoutNode } from "./protocol";

export interface ClaimedRange {
  readonly start: Comment;
  readonly end: Comment;
  readonly layout: Extract<LayoutNode, { kind: "range" }>;
}

export interface PortalPosition {
  readonly parent: Node;
  readonly start: Node | null;
}

export interface ClaimOptions {
  readonly portal?: (layout: Extract<LayoutNode, { kind: "range" }>) => PortalPosition;
}

export class TextRun {
  constructor(
    public node: Text | undefined,
    readonly parent: Node,
    readonly anchor: Node | null,
    readonly parts: string[],
  ) {}

  write(index: number, value: string): void {
    if (this.parts[index] === value) return;
    this.parts[index] = value;
    this.commit();
  }

  commit(): void {
    const text = this.parts.join("");
    if (this.node !== undefined) {
      if (this.node.data !== text) this.node.data = text;
    } else if (text !== "") {
      this.node = this.parent.ownerDocument!.createTextNode(text);
      (this.anchor?.parentNode ?? this.parent).insertBefore(this.node, this.anchor);
    }
  }
}

export class ClaimedText {
  constructor(readonly run: TextRun, public index: number) {}

  get data(): string {
    return this.run.parts[this.index]!;
  }

  set data(value: string) {
    this.run.write(this.index, value);
  }
}

export type ClaimedNode = Element | Comment | ClaimedText;
export type ClaimedLayout = ClaimedNode | ClaimedRange;

export interface ClaimedRoot {
  readonly element: Element;
  readonly layout: Extract<LayoutNode, { kind: "element" }>;
  readonly statics: Map<number, ClaimedNode>;
}

const Namespaces = {
  "": "http://www.w3.org/1999/xhtml",
  svg: "http://www.w3.org/2000/svg",
  math: "http://www.w3.org/1998/Math/MathML",
};

export class ClaimIndex {
  readonly roots = new Map<string, ClaimedRoot>();
  readonly ranges = new Map<string, ClaimedRange>();
  readonly nodes = new Map<LayoutNode, ClaimedLayout>();

  private readonly contexts: Element[] = [];
  constructor(
    readonly root: Element,
    public layout: readonly LayoutNode[],
    readonly ownerTokens: ReadonlyMap<string, string>,
    readonly options: ClaimOptions = {},
  ) {
    let cursor: Node | null = root.firstChild;
    for (const node of layout) {
      if (node.kind === "range" && node.placement !== undefined) {
        const position = options.portal?.(node) ?? this.bodyPortal(node);
        this.scan([node], position.parent, position.start, undefined);
      } else {
        cursor = this.scan([node], root, cursor, undefined);
      }
    }
    if (cursor !== null) throw this.error("unexpected node after application layout");
  }

  validate(): void {
    const current = new ClaimIndex(this.root, this.layout, this.ownerTokens, this.options);
    if (current.contexts.length !== this.contexts.length || current.contexts.some((node, index) => node !== this.contexts[index])) {
      throw this.error("portal transport context changed during preparation");
    }
    for (const [layout, previous] of this.nodes) {
      const next = current.nodes.get(layout);
      if (previous instanceof ClaimedText) {
        if (!(next instanceof ClaimedText) || previous.run.node !== next.run.node) {
          throw this.error("text node identity changed during preparation");
        }
      } else if ("start" in previous) {
        if (next === undefined || !("start" in next) || previous.start !== next.start || previous.end !== next.end) {
          throw this.error(`range ${previous.layout.token} changed during preparation`);
        }
      } else if (previous !== next) {
        throw this.error("claimed node identity changed during preparation");
      }
    }
  }

  private token(id: string): string {
    const token = this.ownerTokens.get(id);
    if (token === undefined) throw this.error(`missing transport owner ${id}`);
    return token;
  }

  unwrapPortals(): void {
    for (const wrapper of this.contexts) wrapper.replaceWith(wrapper.firstChild!);
    this.contexts.length = 0;
  }

  release(): void {
    this.roots.clear();
    this.ranges.clear();
    this.nodes.clear();
    this.contexts.length = 0;
    this.layout = [];
  }

  private scan(layout: readonly LayoutNode[], parent: Node, first: Node | null, root: ClaimedRoot | undefined): Node | null {
    let cursor = first;
    for (let index = 0; index < layout.length; index += 1) {
      const expected = layout[index]!;
      if (expected.kind === "text") {
        const parts: string[] = [];
        const start = index;
        do {
          const text = layout[index] as Extract<LayoutNode, { kind: "text" }>;
          parts.push(text.text);
          index += 1;
        } while (layout[index]?.kind === "text");
        index -= 1;
        const value = parts.join("");
        let node: Text | undefined;
        if (cursor?.nodeType === 3) {
          node = cursor as Text;
          cursor = cursor.nextSibling;
        } else if (value !== "") {
          throw this.error(`missing text in ${parent.nodeName}`);
        }
        const run = new TextRun(node, parent, cursor, parts);
        for (let part = 0; part < parts.length; part += 1) {
          const descriptor = layout[start + part] as Extract<LayoutNode, { kind: "text" }>;
          const claimed = new ClaimedText(run, part);
          this.nodes.set(descriptor, claimed);
          this.staticNode(root, descriptor.index, claimed);
        }
        continue;
      }
      if (expected.kind === "range") {
        const start = this.comment(cursor, `rz:1:${this.token(expected.token)}:start`);
        cursor = this.scan(expected.children, parent, start.nextSibling, root);
        const end = this.comment(cursor, `rz:1:${this.token(expected.token)}:end`);
        const claimed: ClaimedRange = { start, end, layout: expected };
        if (this.ranges.has(expected.token)) throw this.error(`duplicate range ${expected.token}`);
        this.ranges.set(expected.token, claimed);
        this.nodes.set(expected, claimed);
        cursor = end.nextSibling;
        continue;
      }
      if (expected.kind === "marker") {
        const marker = this.comment(cursor, "");
        this.nodes.set(expected, marker);
        this.staticNode(root, expected.index, marker);
        cursor = marker.nextSibling;
        continue;
      }
      if (cursor?.nodeType === 1 && expected.ns !== "" && expected.tag !== (expected.ns === "svg" ? "svg" : "math")) {
        const wrapper = cursor as Element;
        const tag = expected.ns === "svg" ? "svg" : "math";
        if (wrapper.localName === tag && wrapper.namespaceURI === Namespaces[expected.ns]
          && wrapper.getAttribute("data-reze-context") === expected.ns) {
          if (this.scan([expected], wrapper, wrapper.firstChild, root) !== null) throw this.error("unexpected portal transport child");
          this.contexts.push(wrapper);
          cursor = wrapper.nextSibling;
          continue;
        }
      }
      if (cursor?.nodeType !== 1) throw this.error(`missing element ${expected.tag}`);
      const element = cursor as Element;
      if (element.localName !== expected.tag || element.namespaceURI !== Namespaces[expected.ns]) {
        throw this.error(`expected ${expected.ns || "html"}:${expected.tag}, found ${element.namespaceURI}:${element.localName}`);
      }
      let owner = root;
      if (expected.token !== undefined) {
        if (element.getAttribute("data-rz") !== this.token(expected.token) || this.roots.has(expected.token)) {
          throw this.error(`invalid native root ${expected.token}`);
        }
        owner = { element, layout: expected, statics: new Map() };
        this.roots.set(expected.token, owner);
      } else if (element.hasAttribute("data-rz")) {
        throw this.error("unexpected native root token");
      }
      this.nodes.set(expected, element);
      this.staticNode(owner, expected.index, element);
      if (expected.opaque !== true) {
        const container = element.localName === "template" && expected.ns === ""
          ? (element as HTMLTemplateElement).content
          : element;
        const end = this.scan(expected.children, container, container.firstChild, owner);
        if (end !== null) throw this.error(`unexpected child in ${expected.tag}`);
      }
      cursor = element.nextSibling;
    }
    return cursor;
  }

  private staticNode(root: ClaimedRoot | undefined, index: number | undefined, node: ClaimedNode): void {
    if (index === undefined) return;
    if (root === undefined || root.statics.has(index)) throw this.error(`invalid static index ${index}`);
    root.statics.set(index, node);
  }

  private comment(node: Node | null, value: string): Comment {
    if (node?.nodeType !== 8 || (node as Comment).data !== value) throw this.error(`missing comment ${JSON.stringify(value)}`);
    return node as Comment;
  }

  private bodyPortal(layout: Extract<LayoutNode, { kind: "range" }>): PortalPosition {
    if (layout.placement === "inert") {
      const templates = this.root.ownerDocument.querySelectorAll<HTMLTemplateElement>(`template[data-reze-portal="${this.token(layout.token)}"]`);
      if (templates.length !== 1) throw this.error(`missing or duplicate inert portal container ${layout.token}`);
      const parent = templates[0]!.content;
      return { parent, start: parent.firstChild };
    }
    const body = this.root.ownerDocument.body;
    const marker = `rz:1:${this.token(layout.token)}:start`;
    let found: Node | null = null;
    for (let node = body.firstChild; node !== null; node = node.nextSibling) {
      if (node.nodeType !== 8 || (node as Comment).data !== marker) continue;
      if (found !== null) throw this.error(`duplicate portal ${layout.token}`);
      found = node;
    }
    if (found === null) throw this.error(`missing portal ${layout.token}`);
    return { parent: body, start: found };
  }

  private error(message: string): HydrationError {
    return new HydrationError(`DOM layout: ${message}`);
  }
}
