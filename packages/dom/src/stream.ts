import { currentKeyScope, resumeKeyScope, type KeyScope } from "./hydration";
import type { JSX } from "./jsx";

/** `<!--$s:<id>-->` opens the placeholder of a pending async boundary; `<!--/$s-->` closes it. */
export const StreamOpen = "$s:";
export const StreamClose = "/$s";

/**
 * An async component's key scope, captured when its body starts: its await loaders and its
 * content run later, after the scope was left. Stream ids are `scope.id + <await index>`.
 */
export interface StreamBoundary {
  readonly scope: KeyScope;
  readonly stream: BoundaryStream | undefined;
}

/** A server stream that renders async boundaries again as their awaits settle. */
export interface BoundaryStream {
  output(boundary: StreamBoundary, content: () => JSX.Element): JSX.Element;
}

let activeStream: BoundaryStream | undefined;

/** Values of the chunks `applyStreamChunks` applied, by stream id, until an await reads them. */
const chunkValues = new Map<string, unknown>();

/** Runs `fn` with async components rendering into `stream`. */
export function withActiveStream<T>(stream: BoundaryStream, fn: () => T): T {
  const parent = activeStream;
  activeStream = stream;
  try {
    return fn();
  } finally {
    activeStream = parent;
  }
}

/** The boundary of the async component whose body is starting; `undefined` outside key scopes. */
export function streamBoundary(): StreamBoundary | undefined {
  const scope = currentKeyScope();
  return scope && { scope, stream: activeStream };
}

/**
 * An async component's result: `content` computed in the component's key scope, so its keys
 * do not depend on what the parent created meanwhile. A server stream also marks the boundary
 * while an await is pending. On the client only the first read, the hydrating one, resumes
 * the scope.
 */
export function streamOutput(
  boundary: StreamBoundary | undefined,
  content: () => JSX.Element,
): JSX.Element {
  if (boundary === undefined) return content;
  if (boundary.stream) return boundary.stream.output(boundary, content);
  let resumed: KeyScope | undefined = boundary.scope;
  return (() => {
    const scope = resumed;
    resumed = undefined;
    return scope ? resumeKeyScope(scope, content) : content();
  }) as unknown as JSX.Element;
}

/**
 * The promise of await `index` of an async component: the value a streamed chunk carried for
 * it, read once and applied at once, so hydration adopts the chunk's DOM; otherwise `loader()`.
 */
export function streamValue<T>(
  boundary: StreamBoundary | undefined,
  index: number,
  loader: () => T,
): Promise<Awaited<T>> {
  const id = boundary && boundary.scope.id + index;
  if (id === undefined || !chunkValues.has(id)) return Promise.resolve(loader());
  const value = chunkValues.get(id) as Awaited<T>;
  chunkValues.delete(id);
  return new StreamedValue(value) as unknown as Promise<Awaited<T>>;
}

/** A thenable that calls back synchronously: `trackAsync` applies it during hydration. */
class StreamedValue<T> {
  constructor(readonly value: T) {}

  then(onValue: (value: T) => void): void {
    onValue(this.value);
  }
}

/**
 * Moves each `<template data-reze-chunk="<id>">` inside `root` into the placeholder
 * `<!--$s:<id>-->…<!--/$s-->`, replacing the placeholder, and keeps its `data-reze-values` for
 * the async component's `streamValue` (a missing attribute is `undefined`). Chunks may come in
 * any order, also before the chunk holding their placeholder. Placeholders without a chunk stay.
 */
export function applyStreamChunks(root: ParentNode): void {
  const contents = new Map<string, DocumentFragment>();
  for (const chunk of root.querySelectorAll<HTMLTemplateElement>("template[data-reze-chunk]")) {
    const id = chunk.getAttribute("data-reze-chunk")!;
    const values = chunk.getAttribute("data-reze-values");
    chunkValues.set(id, values === null ? undefined : JSON.parse(values));
    contents.set(id, chunk.content);
    chunk.remove();
  }
  if (contents.size) fillPlaceholders(root, contents);
}

function fillPlaceholders(parent: Node, contents: Map<string, DocumentFragment>): void {
  const opens: Comment[] = [];
  const walker = document.createTreeWalker(parent, NodeFilter.SHOW_COMMENT);
  while (walker.nextNode()) {
    const comment = walker.currentNode as Comment;
    if (
      comment.data.startsWith(StreamOpen) &&
      contents.has(comment.data.slice(StreamOpen.length))
    ) {
      opens.push(comment);
    }
  }
  for (const open of opens) {
    const id = open.data.slice(StreamOpen.length);
    const content = contents.get(id);
    if (!content || !parent.contains(open)) continue;
    contents.delete(id);
    fillPlaceholders(content, contents);
    open.parentNode!.insertBefore(content, open);
    let depth = 0;
    for (let node: ChildNode | null = open; node;) {
      const next: ChildNode | null = node.nextSibling;
      const data = node.nodeType === 8 ? (node as Comment).data : "";
      if (data.startsWith(StreamOpen)) depth++;
      else if (data === StreamClose) depth--;
      node.remove();
      if (!depth) break;
      node = next;
    }
  }
}
