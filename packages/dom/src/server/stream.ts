import { isHeadScriptKey } from "../head";
import { hMount } from "../html/helpers";
import { serializeNodes, serializeRangeContent, type HtmlSerializeContext } from "../html/serialize";
import type { HtmlSession } from "../html/session";
import type { HtmlRange } from "../html/tree";
import { islandDescriptor, type ClientModules, type IslandRoot } from "./island";

export interface TemplateParts {
  readonly beforeHeadEnd: string;
  readonly headEndToRoot: string;
  readonly rootEndToBodyEnd: string;
  readonly bodyEndToEnd: string;
}

/** A keyed tag of the document head, emitted unless a `<Head>` tag of the same key shadows it. */
export interface HeadDefault {
  readonly key: string;
  readonly html: string;
}

export interface RenderStreamOptions {
  readonly build: () => unknown;
  readonly session: HtmlSession;
  readonly template: TemplateParts;
  readonly head: string;
  readonly headDefaults: readonly HeadDefault[];
  readonly clientModules: ClientModules;
  /** `shell` ends after the part before the first async boundary; `resume` sends only the parts after that shell. */
  readonly mode: "stream" | "buffered" | "shell" | "resume";
  /** Reports whether the shell still has async work to stream; called once after the shell is serialized. */
  readonly onShell?: (pending: boolean) => void;
  readonly signal?: AbortSignal;
}

const LinkTag = /<link\b([^>]*)>/g;
const Href = /\bhref="([^"]+)"/;
const Ampersand = /&/g;
const Quote = /"/g;
const LessThan = /</g;

function escapeAttribute(value: string): string {
  return value.replace(Ampersand, "&amp;").replace(Quote, "&quot;").replace(LessThan, "&lt;");
}

function isWithin(range: HtmlRange, ancestor: HtmlRange): boolean {
  for (let current: HtmlRange | HtmlRange["parent"] = range; current !== undefined; current = current.parent) {
    if (current === ancestor) return true;
  }
  return false;
}

class Writer {
  private readonly styles = new Set<string>();
  private readonly preloads = new Set<string>();
  private readonly context: HtmlSerializeContext;
  private readonly sentHead = new Map<string, string>();
  private portalCount = 0;

  constructor(
    private readonly session: HtmlSession,
    private readonly modules: ClientModules,
    template: TemplateParts,
    private readonly headDefaults: readonly HeadDefault[],
  ) {
    this.context = { tokenOf: (range) => session.tokenOf(range) };
    for (const [, tag] of template.beforeHeadEnd.matchAll(LinkTag)) {
      const href = tag!.match(Href)?.[1];
      if (href === undefined) continue;
      if (tag!.includes("stylesheet")) this.styles.add(href);
      else if (tag!.includes("modulepreload")) this.preloads.add(href);
    }
  }

  resources(): string {
    let out = "";
    for (const moduleId of this.session.instances.modules) {
      for (const href of this.modules.css(moduleId)) {
        if (this.styles.has(href)) continue;
        this.styles.add(href);
        out += `<link rel="stylesheet" href="${escapeAttribute(href)}">`;
      }
    }
    for (const root of this.session.islands) {
      if (root.sent) continue;
      for (const href of [this.modules.url(root.ref.moduleId), ...this.modules.preload(root.ref.moduleId)]) {
        if (this.preloads.has(href)) continue;
        this.preloads.add(href);
        out += `<link rel="modulepreload" href="${escapeAttribute(href)}">`;
      }
    }
    return out;
  }

  shell(): string {
    return serializeNodes([this.session.mount!], this.context);
  }

  portals(): string {
    let out = "";
    for (const portal of this.session.portals) {
      if (!portal.instance.retired) out += serializeNodes([portal.node], this.context);
    }
    this.portalCount = this.session.portals.length;
    return out;
  }

  latePortals(): string {
    return this.session.portals.length === this.portalCount ? "" : `<template data-rz-patch="p">${this.portals()}</template>\n`;
  }

  private wantedHead(): Map<string, string> {
    const wanted = new Map<string, string>();
    for (const entry of this.headDefaults) wanted.set(entry.key, entry.html);
    for (const tag of this.session.headTags) {
      if (tag.instance.retired) continue;
      const sent = isHeadScriptKey(tag.id) ? this.sentHead.get(tag.id) : undefined;
      wanted.set(tag.id, sent ?? serializeNodes([tag.node], this.context));
    }
    return wanted;
  }

  shellHead(): string {
    let out = "";
    for (const [id, html] of this.wantedHead()) {
      this.sentHead.set(id, html);
      out += html;
    }
    return out;
  }

  lateHead(): string {
    const wanted = this.wantedHead();
    let out = "";
    for (const [id, html] of wanted) {
      if (this.sentHead.get(id) === html) continue;
      this.sentHead.set(id, html);
      out += `<template data-rz-head data-rz-head-key="${escapeAttribute(id)}">${html}</template>\n`;
    }
    for (const id of this.sentHead.keys()) {
      if (wanted.has(id)) continue;
      this.sentHead.delete(id);
      out += `<template data-rz-head data-rz-head-key="${escapeAttribute(id)}"></template>\n`;
    }
    return out;
  }

  patches(): string {
    const session = this.session;
    const dirty = new Set(session.dirtyRanges);
    session.dirtyRanges.clear();
    const emitted: HtmlRange[] = [];
    let out = "";
    for (const range of dirty) {
      let nested = false;
      for (let ancestor = range.parent; ancestor !== undefined; ancestor = ancestor.parent) {
        if (ancestor.kind === "range" && dirty.has(ancestor)) {
          nested = true;
          break;
        }
      }
      if (nested) continue;
      let booted: IslandRoot | undefined;
      for (const root of session.islands) {
        if (root.sent && isWithin(range, root.range)) booted = root;
      }
      if (booted !== undefined) {
        if (process.env.NODE_ENV !== "production") {
          console.warn(`[reze] server change inside a booted island ignored at ${booted.ref.moduleId}#${booted.ref.exportName}`);
        }
        continue;
      }
      out += `<template data-rz-patch="${range.wire}">${serializeRangeContent(range, this.context)}</template>\n`;
      emitted.push(range);
    }
    for (const root of session.islands) {
      if (root.sent && emitted.some((range) => isWithin(root.range, range))) root.sent = false;
    }
    return out;
  }

  descriptors(): string {
    const session = this.session;
    let out = "";
    for (const root of session.islands) {
      if (root.sent || root.range.wire === undefined) continue;
      let complete = true;
      for (const [range, isPending] of session.pendingRanges) {
        if (isPending() && isWithin(range, root.range)) {
          complete = false;
          break;
        }
      }
      if (!complete) continue;
      root.sent = true;
      out += islandDescriptor(session, root, this.modules);
    }
    return out;
  }
}

export async function renderStream(options: RenderStreamOptions): Promise<ReadableStream<Uint8Array>> {
  const { session, template, mode } = options;
  const encoder = new TextEncoder();
  const writer = new Writer(session, options.clientModules, template, options.headDefaults);
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  let closed = false;
  const close = (): void => {
    if (closed) return;
    closed = true;
    session.dispose();
    try {
      controller.close();
    } catch {
      return;
    }
  };
  const stream = new ReadableStream<Uint8Array>({
    start(next) {
      controller = next;
    },
    cancel() {
      closed = true;
      session.dispose();
    },
  });
  const write = (text: string): void => {
    if (text !== "" && !closed) controller.enqueue(encoder.encode(text));
  };
  options.signal?.addEventListener("abort", close, { once: true });

  try {
    session.run(() => hMount(options.build));
    for (;;) {
      session.flush();
      if (mode === "buffered" ? !session.hasPendingWork() : session.isShellReady()) break;
      await session.waitForWork();
    }
    for (const [pending, isPending] of session.pendingRanges) {
      if (!isPending()) continue;
      let owner: HtmlRange | undefined;
      for (const layout of session.layouts) {
        if (isWithin(pending, layout) && (owner === undefined || isWithin(layout, owner))) owner = layout;
      }
      if (owner !== undefined) owner.wire = `${owner.wire}:p`;
    }
    const content = writer.shell();
    const portals = writer.portals();
    const resources = writer.resources();
    const headTags = writer.shellHead();
    if (mode !== "resume") {
      write(
        template.beforeHeadEnd +
          resources +
          options.head +
          headTags +
          template.headEndToRoot +
          content +
          template.rootEndToBodyEnd +
          `<!--rz:p-->${portals}<!--/rz:p--><!--rz-shell-->`,
      );
    }
    const descriptors = writer.descriptors();
    if (mode !== "resume") write(descriptors);
    session.markShellFlushed();
    options.onShell?.(session.hasPendingWork());
  } catch (error) {
    close();
    throw error;
  }

  if (mode === "buffered" || mode === "shell") {
    write(template.bodyEndToEnd);
    close();
    return stream;
  }

  void (async () => {
    let outcome = '{"ok":true}';
    try {
      while (session.hasPendingWork() && !closed) {
        await session.waitForWork();
        session.flush();
        const patches = writer.patches() + writer.latePortals() + writer.lateHead();
        write((patches === "" ? "" : writer.resources()) + patches + writer.descriptors());
      }
    } catch (error) {
      outcome = error instanceof Error && error.name === "ScopeTimeoutError" ? '{"ok":false,"timeout":true}' : '{"ok":false}';
      console.error(error);
    }
    write(`<script type="application/json" data-rz-end>${outcome}</script>\n${template.bodyEndToEnd}`);
    close();
  })();
  return stream;
}
