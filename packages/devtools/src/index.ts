import { getOwner, type Owner } from "@rezejs/signals";
import { setDebugHook, type DebugHook, type DebugNodeKind } from "@rezejs/signals/devtools";

export type OwnerKind = DebugNodeKind | "component";

export interface OwnerTreeNode {
  id: number;
  kind: OwnerKind;
  name: string;
  children: OwnerTreeNode[];
}

export interface SignalWrite {
  id: number;
  name: string;
  value: unknown;
}

export interface Devtools {
  /** The live owner tree: render roots at the top, components grouping what their bodies created. */
  getOwnerTree(): OwnerTreeNode[];
  /** `getOwnerTree()` as indented lines, one per node, repeated leaves folded as `render ×3`. */
  printOwnerTree(): string;
  /** The current value of a signal, computed or render binding; `undefined` for a gone id. */
  getValue(id: number): unknown;
  /** Calls `listener` after each signal write that changed its value; returns the unsubscribe. */
  subscribe(listener: (write: SignalWrite) => void): () => void;
  /** The innermost live component that rendered `element` or one of its ancestors. */
  highlight(element: Node): OwnerTreeNode | undefined;
  /** Stops recording and removes `window.__REZE_DEVTOOLS__`. */
  uninstall(): void;
}

declare global {
  interface Window {
    __REZE_DEVTOOLS__?: Devtools;
  }
}

interface DebugRecord {
  id: number;
  kind: OwnerKind;
  name: string;
  read: () => unknown;
  /** Components only: the owner their body runs under. */
  owner: Owner | undefined;
  parent: DebugRecord | undefined;
  children: Set<DebugRecord>;
}

const OwnedByRun: ReadonlySet<OwnerKind> = new Set(["signal", "computed", "component"]);

/**
 * Starts recording every reactive node and component created from now on and exposes the
 * inspection API as `window.__REZE_DEVTOOLS__`. Development builds only: production builds
 * compile the runtime's hook calls away, so the tree stays empty there.
 */
export function installDevtools(): Devtools {
  let nextId = 0;
  const byNode = new WeakMap<Owner, DebugRecord>();
  const byId = new Map<number, DebugRecord>();
  const top = new Set<DebugRecord>();
  const components: DebugRecord[] = [];
  const renderedBy = new WeakMap<Node, DebugRecord>();
  const listeners = new Set<(write: SignalWrite) => void>();
  const countByKind = new Map<OwnerKind, number>();

  const defaultName = (kind: OwnerKind): string => {
    if (kind !== "signal" && kind !== "computed") return kind;
    const count = (countByKind.get(kind) ?? 0) + 1;
    countByKind.set(kind, count);
    return `${kind}#${count}`;
  };

  const currentParent = (): DebugRecord | undefined => {
    const owner = getOwner();
    const component = components[components.length - 1];
    if (component !== undefined && component.owner === owner) return component;
    return owner === undefined ? undefined : byNode.get(owner);
  };

  const add = (
    kind: OwnerKind,
    name: string | undefined,
    read: () => unknown,
    owner: Owner | undefined,
  ): DebugRecord => {
    const parent = currentParent();
    const record: DebugRecord = {
      id: ++nextId,
      kind,
      name: name || defaultName(kind),
      read,
      owner,
      parent,
      children: new Set(),
    };
    (parent?.children ?? top).add(record);
    byId.set(record.id, record);
    return record;
  };

  const drop = (record: DebugRecord): void => {
    (record.parent?.children ?? top).delete(record);
    const forget = (gone: DebugRecord): void => {
      byId.delete(gone.id);
      for (const child of gone.children) forget(child);
    };
    forget(record);
  };

  const hook: DebugHook = {
    created(node, kind, name, read) {
      byNode.set(node, add(kind, name, read, undefined));
    },
    rerunning(node) {
      const record = byNode.get(node);
      if (record === undefined) return;
      for (const child of record.children) {
        if (OwnedByRun.has(child.kind)) drop(child);
      }
    },
    disposed(node) {
      const record = byNode.get(node);
      if (record !== undefined && byId.has(record.id)) drop(record);
    },
    written(node) {
      const record = byNode.get(node);
      if (record === undefined || !byId.has(record.id)) return;
      const write = { id: record.id, name: record.name, value: record.read() };
      for (const listener of listeners) listener(write);
    },
    component(name, run) {
      const record = add("component", name || "Anonymous", () => undefined, getOwner());
      components.push(record);
      let rendered: unknown;
      try {
        rendered = run();
      } finally {
        components.pop();
      }
      for (const node of Array.isArray(rendered) ? rendered : [rendered]) {
        if (node instanceof Node && !renderedBy.has(node)) renderedBy.set(node, record);
      }
      return rendered as ReturnType<typeof run>;
    },
  };

  const snapshot = (record: DebugRecord): OwnerTreeNode => ({
    id: record.id,
    kind: record.kind,
    name: record.name,
    children: [...record.children].map(snapshot),
  });

  const devtools: Devtools = {
    getOwnerTree: () => [...top].map(snapshot),
    printOwnerTree: () => printTree(devtools.getOwnerTree(), 0).join("\n"),
    getValue: (id) => byId.get(id)?.read(),
    subscribe(listener) {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    highlight(element) {
      for (let node: Node | null = element; node !== null; node = node.parentNode) {
        const record = renderedBy.get(node);
        if (record !== undefined && byId.has(record.id)) return snapshot(record);
      }
      return undefined;
    },
    uninstall() {
      setDebugHook(undefined);
      if (typeof window !== "undefined" && window.__REZE_DEVTOOLS__ === devtools) {
        delete window.__REZE_DEVTOOLS__;
      }
    },
  };
  setDebugHook(hook);
  if (typeof window !== "undefined") window.__REZE_DEVTOOLS__ = devtools;
  return devtools;
}

function label(node: OwnerTreeNode): string {
  return node.kind === "component" || node.kind === node.name
    ? node.name
    : `${node.kind} ${node.name}`;
}

function printTree(nodes: OwnerTreeNode[], depth: number): string[] {
  const lines: string[] = [];
  const indent = "  ".repeat(depth);
  for (let i = 0; i < nodes.length;) {
    const node = nodes[i]!;
    let repeats = 1;
    while (
      node.children.length === 0 &&
      nodes[i + repeats]?.children.length === 0 &&
      label(nodes[i + repeats]!) === label(node)
    ) {
      repeats++;
    }
    lines.push(`${indent}${label(node)}${repeats > 1 ? ` ×${repeats}` : ""}`);
    lines.push(...printTree(node.children, depth + 1));
    i += repeats;
  }
  return lines;
}
