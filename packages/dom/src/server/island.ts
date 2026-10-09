import type { Instance } from "../html/instances";
import type { HtmlSession } from "../html/session";
import type { HtmlRange } from "../html/tree";
import { CodecError, encodePayloadJson, FrameEncoder, type WireValue } from "../wire/codec";

export interface ComponentRef {
  readonly moduleId: string;
  readonly exportName: string;
  readonly clientWork: boolean;
}

export interface ComponentEntry {
  readonly component: (props: never) => unknown;
  readonly props: unknown;
  readonly range: HtmlRange;
}

export interface Seed {
  values: unknown[];
  rejection: { readonly error: unknown } | undefined;
}

export interface IslandRoot {
  readonly prefix: string;
  readonly range: HtmlRange;
  readonly instance: Instance;
  readonly ref: ComponentRef;
  readonly encoder: FrameEncoder;
  readonly props: WireValue;
  readonly slots: Record<string, HtmlRange>;
  readonly trigger: string | undefined;
  readonly options: unknown;
  readonly seeds: Map<string, Seed>;
  readonly occurrences: Map<string, number>;
  nextId: number;
  sent: boolean;
}

export interface ClientModules {
  url(moduleId: string): string;
  css(moduleId: string): readonly string[];
  preload(moduleId: string): readonly string[];
}

const refs = new WeakMap<object, ComponentRef>();
export const builtinComponents = new WeakSet<object>();

export function hDefineComponent(component: (props: never) => unknown, moduleId: string, exportName: string, clientWork: 0 | 1): void {
  refs.set(component, { moduleId, exportName, clientWork: clientWork === 1 });
}

export function componentRefOf(component: object): ComponentRef | undefined {
  return refs.get(component);
}

export function hStaticComponent<T extends object>(component: T): T {
  builtinComponents.add(component);
  return component;
}

const NodeKinds: Record<string, true> = { element: true, text: true, marker: true, range: true };

function isNodeValue(value: unknown): boolean {
  if (Array.isArray(value)) return value.some(isNodeValue);
  return (
    typeof value === "object" && value !== null && "meta" in value && "kind" in value && Object.hasOwn(NodeKinds, value.kind as string)
  );
}

export function rejectRenderedNode(value: object): string | undefined {
  return isNodeValue(value) ? "rendered node" : undefined;
}

export function encodeProps(
  session: HtmlSession,
  props: Record<string, unknown>,
  label: string,
): { encoder: FrameEncoder; wire: WireValue } {
  const encoder = new FrameEncoder(session.pathname, rejectRenderedNode);
  return { encoder, wire: encoder.capture(label, [props]).values[0]! };
}

export function createIslandRoot(
  session: HtmlSession,
  instance: Instance,
  entry: ComponentEntry,
  ref: ComponentRef,
  makeSlot: ((build: () => unknown) => HtmlRange) | undefined,
  options?: { trigger: string; options: unknown },
): { root: IslandRoot; view: unknown } {
  const props = entry.props as Record<PropertyKey, unknown>;
  const plain: Record<string, unknown> = {};
  const view: Record<PropertyKey, unknown> = {};
  const slotKeys: string[] = [];
  for (const key of Reflect.ownKeys(props)) {
    if (typeof key === "symbol") throw new CodecError(`reze: unsupported value symbol-keyed prop on ${ref.exportName}`);
    if (key === "children") {
      slotKeys.push(key);
      continue;
    }
    const value = props[key];
    if (isNodeValue(value)) {
      slotKeys.push(key);
      view[key] = value;
    } else {
      plain[key] = value;
      Object.defineProperty(view, key, Reflect.getOwnPropertyDescriptor(props, key)!);
    }
  }
  if (makeSlot === undefined && slotKeys.length !== 0) {
    throw new CodecError(`reze: unsupported value rendered content in prop "${slotKeys[0]}" of ${ref.exportName}`);
  }
  const { encoder, wire } = encodeProps(session, plain, `island:${ref.exportName}`);
  const slots: Record<string, HtmlRange> = {};
  for (const key of slotKeys) {
    const content = view[key];
    const slot = makeSlot!(key === "children" ? () => props[key] : () => content);
    slot.marked = true;
    slots[key] = slot;
    view[key] = slot;
  }
  const root: IslandRoot = {
    prefix: session.islandPrefix(),
    range: entry.range,
    instance,
    ref,
    encoder,
    props: wire,
    slots,
    trigger: options?.trigger,
    options: options?.options,
    seeds: new Map(),
    occurrences: new Map(),
    nextId: 0,
    sent: false,
  };
  return { root, view };
}

export function adoptIslandRoot(session: HtmlSession, instance: Instance, root: IslandRoot): void {
  root.range.marked = true;
  session.islands.add(root);
  const pending = [instance];
  while (pending.length !== 0) {
    const current = pending.pop()!;
    if (current.island !== undefined && current.island !== root) session.islands.delete(current.island);
    current.island = root;
    for (const child of current.children) pending.push(child);
  }
}

export function escalateIsland(session: HtmlSession, start: Instance | undefined): boolean {
  for (let instance = start; instance !== undefined; instance = instance.parent) {
    const entry = instance.component;
    if (entry === undefined) continue;
    const ref = componentRefOf(entry.component);
    if (ref === undefined) continue;
    const props = entry.props as Record<PropertyKey, unknown>;
    if (typeof props !== "object" || props === null || Object.hasOwn(props, "children")) continue;
    const plain: Record<string, unknown> = {};
    let encodable = true;
    for (const key of Reflect.ownKeys(props)) {
      if (typeof key === "symbol") {
        encodable = false;
        break;
      }
      plain[key] = props[key];
    }
    if (!encodable) continue;
    let encoded: { encoder: FrameEncoder; wire: WireValue };
    try {
      encoded = encodeProps(session, plain, `island:${ref.exportName}`);
    } catch (error) {
      if (error instanceof CodecError) continue;
      throw error;
    }
    const root: IslandRoot = {
      prefix: session.islandPrefix(),
      range: entry.range,
      instance,
      ref,
      encoder: encoded.encoder,
      props: encoded.wire,
      slots: {},
      trigger: undefined,
      options: undefined,
      seeds: new Map(),
      occurrences: new Map(),
      nextId: 0,
      sent: false,
    };
    adoptIslandRoot(session, instance, root);
    if (process.env.NODE_ENV !== "production" && session.shellFlushed && entry.range.wire !== undefined) {
      console.warn(`[reze] island boundary moved to ${ref.moduleId}#${ref.exportName} after streaming started`);
    }
    return true;
  }
  return false;
}

export function beginSeed(instance: Instance, siteKey: string): Seed | undefined {
  const island = instance.island;
  if (island === undefined) return undefined;
  const occurrence = island.occurrences.get(siteKey) ?? 0;
  island.occurrences.set(siteKey, occurrence + 1);
  const seed: Seed = { values: [], rejection: undefined };
  island.seeds.set(`${siteKey}:${occurrence}`, seed);
  return seed;
}

export function islandDescriptor(session: HtmlSession, root: IslandRoot, modules: ClientModules): string {
  const keys: string[] = [];
  const values: WireValue[] = [];
  for (const [key, seed] of root.seeds) {
    try {
      values.push(root.encoder.capture(`seed:${key}`, [{ values: seed.values, rejection: seed.rejection }]).values[0]!);
      keys.push(key);
    } catch (error) {
      if (!(error instanceof CodecError)) throw error;
      if (process.env.NODE_ENV !== "production") console.warn(`[reze] seed ${key} cannot be sent to the browser and will be fetched again`);
    }
  }
  const slots: Record<string, string> = {};
  for (const [name, slot] of Object.entries(root.slots)) if (slot.wire !== undefined) slots[name] = slot.wire;
  const descriptor = {
    m: modules.url(root.ref.moduleId),
    n: root.ref.moduleId,
    e: root.ref.exportName,
    i: root.prefix,
    f: root.encoder.frames,
    p: root.props,
    ...(Object.keys(slots).length === 0 ? {} : { l: slots }),
    ...(keys.length === 0 ? {} : { k: keys, v: values }),
    ...(root.trigger === undefined ? {} : { t: root.trigger, o: root.options ?? null }),
  };
  return `<script type="application/json" data-rz-island="${session.tokenOf(root.range)}">${encodePayloadJson(descriptor)}</script>\n`;
}
