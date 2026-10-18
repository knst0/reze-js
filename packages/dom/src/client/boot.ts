import { provideContext } from "@rezejs/signals";
import { IslandContext, type IslandState, type Seed } from "@rezejs/signals/internal/resource";
import { renderRoot } from "@rezejs/signals/internal/scope";

import { createComponent } from "../component";
import { insert } from "../insert";
import { armIsland, type IslandOptions, type IslandTrigger } from "../island";
import type { JSX } from "../jsx";
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
}

export interface BootOptions {
  readonly wrap?: <T>(render: () => T) => T;
  readonly islands?: Readonly<Record<string, () => Promise<unknown>>>;
}

export interface RangeLocator {
  find(token: string): { start: Comment; end: Comment } | undefined;
}

const booted = new Map<Comment, () => void>();

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

function nodesBetween(start: Comment, end: Comment): Node[] {
  const nodes: Node[] = [];
  for (let node = start.nextSibling; node !== null && node !== end; node = node.nextSibling) nodes.push(node);
  return nodes;
}

function hostBetween(start: Comment, end: Comment): Element | undefined {
  return nodesBetween(start, end).find((node): node is Element => node.nodeType === 1);
}

async function run(
  descriptor: IslandDescriptor,
  locator: RangeLocator,
  start: Comment,
  end: Comment,
  options: BootOptions,
  releaseClicks: () => HeldClick | undefined,
): Promise<{ dispose: () => void; held: HeldClick | undefined; target: Element | undefined }> {
  const load = options.islands?.[descriptor.n];
  const module = (await (load === undefined ? import(/* @vite-ignore */ descriptor.m) : load())) as Record<
    string,
    (props: Record<string, unknown>) => JSX.Element
  >;
  const component = module[descriptor.e];
  if (typeof component !== "function") throw new Error(`module ${descriptor.m} does not export ${descriptor.e}`);
  const decoder = new FrameDecoder(descriptor.f);
  for (let index = 0; index < descriptor.f.length; index += 1) decoder.apply(index);
  const props = { ...(decoder.read(descriptor.p) as Record<string, unknown>) };
  const seeds = new Map<string, Seed>();
  descriptor.k?.forEach((key, index) => seeds.set(key, decoder.read(descriptor.v![index]!) as Seed));
  if (descriptor.l !== undefined) {
    for (const [name, token] of Object.entries(descriptor.l)) {
      const slot = locator.find(token);
      props[name] = slot === undefined ? [] : nodesBetween(slot.start, slot.end);
    }
  }
  const state: IslandState = { prefix: descriptor.i, seeds, occurrences: new Map(), nextId: 0 };
  const old = nodesBetween(start, end).filter((node) => !isSlotNode(props, node));
  const snapshot = snapshotForms(old, start.ownerDocument.activeElement);
  const held = releaseClicks();
  const heldAt = held === undefined ? undefined : clickPosition(old, held.target);
  for (const node of old) node.parentNode!.removeChild(node);
  const render = (): unknown => provideContext(IslandContext, state, () => createComponent(component, props));
  const dispose = renderRoot((dispose) => {
    insert(end.parentNode!, options.wrap === undefined ? render() : options.wrap(render), end);
    return dispose;
  });
  const rendered = nodesBetween(start, end);
  restoreForms(rendered, snapshot);
  return { dispose, held, target: heldAt === undefined ? held?.target : elementAt(rendered, heldAt) };
}

function isSlotNode(props: Record<string, unknown>, node: Node): boolean {
  for (const value of Object.values(props)) {
    if (Array.isArray(value) && value.includes(node)) return true;
  }
  return false;
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
  let cancelled = false;
  let disarm: (() => void) | undefined;
  let disposeRender: (() => void) | undefined;
  let releaseClicks: (() => HeldClick | undefined) | undefined;
  const dispose = (): void => {
    cancelled = true;
    booted.delete(start);
    disarm?.();
    releaseClicks?.();
    disposeRender?.();
  };
  booted.set(start, dispose);
  const boot = (): void => {
    releaseClicks = holdClicks(nodesBetween(start, end));
    run(descriptor, locator, start, end, options, releaseClicks).then(
      (island) => {
        if (cancelled) {
          island.dispose();
          return;
        }
        disposeRender = island.dispose;
        replayClick(island.target, island.held);
      },
      (error: unknown) => {
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
