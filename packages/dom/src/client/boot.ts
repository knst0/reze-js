import { getOwner, provideContext, runWithOwner } from "@rezejs/signals";
import { IslandContext, type IslandState, type Seed } from "@rezejs/signals/internal/resource";
import { renderRoot, type ReactiveNode } from "@rezejs/signals/internal/scope";

import { createComponent } from "../component";
import { insert } from "../insert";
import { armIsland, type IslandOptions, type IslandTrigger } from "../island";
import type { JSX } from "../jsx";
import { AdoptionError, beginAdoption, endAdoption } from "../template";
import { FrameDecoder, type WireValue } from "../wire/codec";
import { clickPosition, elementAt, holdClicks, replayClick, type HeldClick } from "./clicks";
import { restoreForms, snapshotForms } from "./forms";

export interface IslandDescriptor {
  readonly m: string;
  readonly n: string;
  readonly e: string;
  readonly i: string;
  readonly f: readonly unknown[];
  readonly p: WireValue;
  readonly l?: Readonly<Record<string, string>>;
  readonly k?: readonly string[];
  readonly v?: readonly WireValue[];
  readonly t?: IslandTrigger;
  readonly o?: IslandOptions | null;
  readonly a?: number;
}

export interface BootOptions {
  readonly wrap?: <T>(render: () => T) => T;
  readonly islands?: Readonly<Record<string, () => Promise<unknown>>>;
}

export interface RangeLocator {
  find(token: string): { start: Comment; end: Comment } | undefined;
}

interface Slot {
  readonly start: Comment;
  readonly end: Comment;
  readonly opened: PromiseWithResolvers<ReactiveNode | undefined>;
}

const booted = new Map<Comment, () => void>();
const slots = new Set<Slot>();

function contains(start: Comment, end: Comment, node: Node): boolean {
  return (
    start !== node &&
    (start.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 &&
    (end.compareDocumentPosition(node) & Node.DOCUMENT_POSITION_PRECEDING) !== 0
  );
}

export function disposeIslandsBetween(start: Comment, end: Comment): void {
  for (const [marker, dispose] of booted) {
    if (contains(start, end, marker)) {
      booted.delete(marker);
      dispose();
    }
  }
}

export function disposeIslandsIn(root: Node): void {
  for (const [marker, dispose] of booted) {
    if (root.contains(marker)) {
      booted.delete(marker);
      dispose();
    }
  }
}

function stripStreamMarkers(start: Comment, end: Comment): void {
  const document = start.ownerDocument!;
  const walker = document.createTreeWalker(document.documentElement, NodeFilter.SHOW_COMMENT);
  const doomed: Comment[] = [];
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    const comment = node as Comment;
    if (comment === start || comment === end || !contains(start, end, comment)) continue;
    if (comment.data.startsWith("rz:") || comment.data.startsWith("/rz:")) doomed.push(comment);
  }
  for (const comment of doomed) comment.parentNode?.removeChild(comment);
}

function nodesBetween(start: Comment, end: Comment): Node[] {
  const nodes: Node[] = [];
  for (let node = start.nextSibling; node !== null && node !== end; node = node.nextSibling) nodes.push(node);
  return nodes;
}

function enclosingSlot(node: Comment): Promise<ReactiveNode | undefined> | undefined {
  let found: Slot | undefined;
  for (const slot of slots) {
    if (contains(slot.start, slot.end, node) && (found === undefined || contains(found.start, found.end, slot.start))) found = slot;
  }
  return found?.opened.promise;
}

function openSlots(descriptor: IslandDescriptor, locator: RangeLocator): Map<string, Slot> {
  const opened = new Map<string, Slot>();
  if (descriptor.l === undefined) return opened;
  for (const [name, token] of Object.entries(descriptor.l)) {
    const range = locator.find(token);
    if (range === undefined) continue;
    const slot: Slot = { start: range.start, end: range.end, opened: Promise.withResolvers() };
    slots.add(slot);
    opened.set(name, slot);
  }
  return opened;
}

function closeSlots(opened: Map<string, Slot>): void {
  for (const slot of opened.values()) {
    slots.delete(slot);
    slot.opened.resolve(undefined);
  }
}

function hostBetween(start: Comment, end: Comment): Element | undefined {
  return nodesBetween(start, end).find((node): node is Element => node.nodeType === 1);
}

async function run(
  descriptor: IslandDescriptor,
  opened: Map<string, Slot>,
  parent: Promise<ReactiveNode | undefined> | undefined,
  start: Comment,
  end: Comment,
  options: BootOptions,
  releaseClicks: () => HeldClick | undefined,
): Promise<{ dispose: () => void; held: HeldClick | undefined; target: Element | undefined }> {
  const load = options.islands?.[descriptor.n];
  const [loaded, owner] = await Promise.all([load === undefined ? import(/* @vite-ignore */ descriptor.m) : load(), parent]);
  const module = loaded as Record<string, (props: Record<string, unknown>) => JSX.Element>;
  const component = module[descriptor.e];
  if (typeof component !== "function") throw new Error(`module ${descriptor.m} does not export ${descriptor.e}`);
  const decoder = new FrameDecoder(descriptor.f);
  for (let index = 0; index < descriptor.f.length; index += 1) decoder.apply(index);
  const props = { ...(decoder.read(descriptor.p) as Record<string, unknown>) };
  const seeds = new Map<string, Seed>();
  descriptor.k?.forEach((key, index) => seeds.set(key, decoder.read(descriptor.v![index]!) as Seed));
  const slotNodes = new Set<Node>();
  for (const name of Object.keys(descriptor.l ?? {})) {
    const slot = opened.get(name);
    if (slot === undefined) {
      props[name] = [];
      continue;
    }
    const nodes = nodesBetween(slot.start, slot.end);
    for (const node of nodes) slotNodes.add(node);
    Object.defineProperty(props, name, {
      configurable: true,
      enumerable: true,
      get: () => {
        slot.opened.resolve(getOwner());
        return nodes;
      },
    });
  }
  const state: IslandState = { prefix: descriptor.i, seeds, occurrences: new Map(), nextId: 0 };
  const render = (): unknown => provideContext(IslandContext, state, () => createComponent(component, props));
  const held = releaseClicks();
  if (descriptor.a === 1 && slotNodes.size === 0) {
    stripStreamMarkers(start, end);
    const roots = nodesBetween(start, end);
    if (roots.length > 0) {
      const adoption = beginAdoption(roots);
      let dispose: (() => void) | undefined;
      try {
        runWithOwner(owner, () =>
          renderRoot((stop) => {
            dispose = stop;
            return options.wrap === undefined ? render() : options.wrap(render);
          }),
        );
        if (adoption.index !== roots.length) {
          throw new AdoptionError(`the server sent ${roots.length} roots and the component claimed ${adoption.index}`);
        }
        endAdoption();
        return { dispose: dispose!, held, target: held?.target };
      } catch (error) {
        endAdoption();
        dispose?.();
        if (!(error instanceof AdoptionError)) throw error;
      }
    }
  }
  const old = nodesBetween(start, end).filter((node) => !slotNodes.has(node));
  const snapshot = snapshotForms(old, start.ownerDocument.activeElement);
  const heldAt = held === undefined ? undefined : clickPosition(old, held.target);
  for (const node of old) node.parentNode!.removeChild(node);
  const dispose = runWithOwner(owner, () =>
    renderRoot((dispose) => {
      insert(end.parentNode!, options.wrap === undefined ? render() : options.wrap(render), end);
      return dispose;
    }),
  );
  const rendered = nodesBetween(start, end);
  restoreForms(rendered, snapshot);
  return { dispose, held, target: heldAt === undefined ? held?.target : elementAt(rendered, heldAt) };
}

export function bootIsland(token: string, descriptor: IslandDescriptor, locator: RangeLocator, options: BootOptions = {}): void {
  const range = locator.find(token);
  if (range === undefined) {
    console.error(`[reze] island descriptor for unknown range ${token}`);
    return;
  }
  const { start, end } = range;
  disposeIslandsBetween(start, end);
  booted.get(start)?.();
  const parent = enclosingSlot(start);
  const opened = openSlots(descriptor, locator);
  let cancelled = false;
  let disarm: (() => void) | undefined;
  let disposeRender: (() => void) | undefined;
  let releaseClicks: (() => HeldClick | undefined) | undefined;
  const dispose = (): void => {
    cancelled = true;
    booted.delete(start);
    closeSlots(opened);
    disarm?.();
    releaseClicks?.();
    disposeRender?.();
  };
  booted.set(start, dispose);
  const boot = (): void => {
    releaseClicks = holdClicks(nodesBetween(start, end));
    run(descriptor, opened, parent, start, end, options, releaseClicks).then(
      (island) => {
        if (cancelled) {
          island.dispose();
          return;
        }
        disposeRender = island.dispose;
        replayClick(island.target, island.held);
      },
      (error: unknown) => {
        closeSlots(opened);
        const held = releaseClicks!();
        console.error(`[reze] island ${descriptor.e} from ${descriptor.m} failed`, error);
        if (!cancelled) replayClick(held?.target, held);
      },
    );
  };
  const trigger = descriptor.t;
  if (trigger === undefined || trigger === "eager") {
    boot();
    return;
  }
  const host = hostBetween(start, end);
  disarm = renderRoot((disposeRoot) => {
    armIsland(trigger, () => boot(), descriptor.o ?? undefined, host);
    return disposeRoot;
  });
}
