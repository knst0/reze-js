import type { SourceSite } from "@rezejs/signals/internal/scope";

import { FrameDecoder, type GraphFrame, type WireValue } from "./codec";

export type NamespaceKey = "" | "svg" | "math";
export type RangeKind = "branch" | "list" | "row" | "portal" | "island" | "fragment" | "component" | "async" | "insertion";

export interface StaticLayout {
  readonly parent: number | null;
  readonly children: readonly number[];
  readonly kind: "element" | "text" | "marker";
  readonly tag?: string;
  readonly ns?: NamespaceKey;
  readonly text?: string;
  readonly attrs?: readonly (readonly [string, string | null])[];
  readonly dynamic?: true;
}

export interface ElementLayout {
  readonly tag: string;
  readonly ns: NamespaceKey;
  readonly nodes: readonly StaticLayout[];
  readonly inserts: readonly { readonly slot: number; readonly parent: number; readonly anchor: "only" | "end" | number }[];
}

export interface Site extends SourceSite {
  readonly layout?: ElementLayout | { readonly dynamic: true } | { readonly range: RangeKind };
}

export interface OwnerRecord {
  readonly id: string;
  readonly parentId?: string;
  readonly retired: boolean;
}

export interface RouteRecord {
  readonly id: string;
  readonly params: WireValue;
  readonly hasData: boolean;
  readonly data?: WireValue;
  readonly frame: number;
}

export interface ResourceStateRecord {
  readonly pending: boolean;
  readonly hasResolved: boolean;
  readonly resolved?: WireValue;
  readonly hasRejection: boolean;
  readonly rejection?: WireValue;
  readonly frame: number;
}

export interface ResourceRecord {
  readonly id: string;
  readonly ownerId: string;
  readonly seeded: boolean;
  readonly states: readonly ResourceStateRecord[];
}

export interface AwaitRecord {
  readonly id: string;
  readonly ownerId: string;
  readonly occurrence: number;
  readonly status: "resolved" | "rejected" | "thrown";
  readonly value: WireValue;
  readonly frame: number;
}

export interface Handoff {
  readonly seq: number;
  readonly kind: "route" | "resource" | "await" | "flush" | "checkpoint";
  readonly id: string;
  readonly ownerId: string;
  readonly index?: number;
  readonly delivery: "inline" | "scheduled";
}

export type LayoutNode =
  | {
      readonly kind: "element";
      readonly tag: string;
      readonly ns: NamespaceKey;
      readonly token?: string;
      readonly site?: string;
      readonly index?: number;
      readonly opaque?: true;
      readonly children: readonly LayoutNode[];
    }
  | { readonly kind: "text"; readonly text: string; readonly index?: number }
  | { readonly kind: "marker"; readonly index?: number }
  | {
      readonly kind: "range";
      readonly token: string;
      readonly ownerId: string;
      readonly site?: string;
      readonly range: RangeKind;
      readonly placement?: "body" | "inert";
      readonly children: readonly LayoutNode[];
    };

export interface HeadDefaults {
  readonly title?: string;
  readonly description?: string;
  readonly canonical?: string;
  readonly robots?: string;
}

export interface HydrationPayload {
  readonly version: 1;
  readonly buildId: string;
  readonly rootId: string;
  readonly pathname: string;
  readonly timeoutMs: number;
  readonly headDefaults: HeadDefaults;
  readonly routes: readonly RouteRecord[];
  readonly resources: readonly ResourceRecord[];
  readonly awaitSlots: readonly AwaitRecord[];
  readonly frames: readonly GraphFrame[];
  readonly owners: readonly OwnerRecord[];
  readonly handoffs: readonly Handoff[];
  readonly modules: readonly string[];
  readonly layout: readonly LayoutNode[];
}

export function createOwnerTokens(owners: readonly OwnerRecord[]): ReadonlyMap<string, string> {
  const tokens = new Map<string, string>();
  for (let index = 0; index < owners.length; index++) tokens.set(owners[index]!.id, index.toString(36));
  return tokens;
}

function ownerToken(tokens: ReadonlyMap<string, string>, id: string): string {
  const token = tokens.get(id);
  if (token === undefined) throw new HydrationError(`unknown transport owner ${id}`);
  return token;
}

function compactLayout(node: LayoutNode, tokens: ReadonlyMap<string, string>): LayoutNode {
  if (node.kind === "text" || node.kind === "marker") return node;
  const children = node.children.map((child) => compactLayout(child, tokens));
  if (node.kind === "range") {
    return { ...node, token: ownerToken(tokens, node.token), ownerId: ownerToken(tokens, node.ownerId), children };
  }
  return { ...node, ...(node.token === undefined ? {} : { token: ownerToken(tokens, node.token) }), children };
}

export function serializePayload(payload: HydrationPayload, tokens = createOwnerTokens(payload.owners)): string {
  const compact = {
    ...payload,
    owners: payload.owners.map((owner) =>
      owner.parentId === undefined
        ? owner
        : {
            id: owner.id.slice(owner.parentId.length + 1),
            parentId: ownerToken(tokens, owner.parentId),
            retired: owner.retired,
          },
    ),
    resources: payload.resources.map((resource) => ({ ...resource, ownerId: ownerToken(tokens, resource.ownerId) })),
    awaitSlots: payload.awaitSlots.map((slot) => ({ ...slot, ownerId: ownerToken(tokens, slot.ownerId) })),
    handoffs: payload.handoffs.map((handoff) => ({ ...handoff, ownerId: ownerToken(tokens, handoff.ownerId) })),
    layout: payload.layout.map((node) => compactLayout(node, tokens)),
  };
  return JSON.stringify(compact).replace(
    /[<>&\u2028\u2029]/g,
    (character) => `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`,
  );
}

export class HydrationError extends Error {
  constructor(message: string, site?: SourceSite) {
    super(site === undefined ? `reze: ${message}` : `reze: ${message} at ${site.module}:${site.line}:${site.column} (site ${site.key})`);
    this.name = "HydrationError";
  }
}

const TOKEN = /^[A-Za-z0-9_.]+$/;
const ROOT_ID = /^[A-Za-z_][A-Za-z0-9_-]*$/;
const SITE_KEY = /^[a-f0-9]{16}_[0-9a-z]+$/;
const RANGE_KINDS = new Set<unknown>(["branch", "list", "row", "portal", "island", "fragment", "component", "async", "insertion"]);

export function parsePayload(text: string): { payload: HydrationPayload; decoder: FrameDecoder } {
  let input: unknown;
  try {
    input = JSON.parse(text);
  } catch {
    throw new HydrationError("hydration state is not valid JSON");
  }
  const data = record(input, "payload");
  requireCondition(data.version === 1, "unsupported hydration protocol version");
  const ids: string[] = [];
  for (const raw of arrayValue(data.owners, "owners")) {
    const owner = record(raw, "owner");
    const key = tokenValue(owner.id, "owner key");
    if (Object.hasOwn(owner, "parentId")) {
      const parent = expandOwnerToken(owner.parentId, ids);
      owner.parentId = parent;
      owner.id = `${parent}.${key}`;
    }
    ids.push(owner.id as string);
  }
  for (const key of ["resources", "awaitSlots", "handoffs"]) {
    for (const raw of arrayValue(data[key], key)) {
      const item = record(raw, key);
      item.ownerId = expandOwnerToken(item.ownerId, ids);
    }
  }
  expandLayoutTokens(data.layout, ids);
  return validatePayload(input);
}

function expandOwnerToken(value: unknown, ids: readonly string[]): string {
  const token = textValue(value, "owner token");
  const index = Number.parseInt(token, 36);
  requireCondition(
    Number.isSafeInteger(index) && index >= 0 && index < ids.length && index.toString(36) === token,
    `invalid owner token ${token}`,
  );
  return ids[index]!;
}

function expandLayoutTokens(input: unknown, ids: readonly string[]): void {
  for (const raw of arrayValue(input, "layout")) {
    const node = record(raw, "layout node");
    if (node.kind !== "element" && node.kind !== "range") continue;
    if (Object.hasOwn(node, "token")) node.token = expandOwnerToken(node.token, ids);
    if (node.kind === "range") node.ownerId = expandOwnerToken(node.ownerId, ids);
    expandLayoutTokens(node.children, ids);
  }
}

export function validatePayload(input: unknown): { payload: HydrationPayload; decoder: FrameDecoder } {
  const data = record(input, "payload");
  requireCondition(data.version === 1, "unsupported hydration protocol version");
  const buildId = textValue(data.buildId, "buildId");
  requireCondition(
    !buildId.startsWith("/") &&
      !buildId.includes("\\") &&
      !/[?#:]/.test(buildId) &&
      buildId.split("/").every((part) => part !== "" && part !== "." && part !== ".."),
    "invalid bootstrap buildId",
  );
  requireCondition(ROOT_ID.test(textValue(data.rootId, "rootId")), "invalid mount root id");
  requireCondition(textValue(data.pathname, "pathname").startsWith("/") && !/[?#]/.test(data.pathname as string), "invalid page pathname");
  requireCondition(
    typeof data.timeoutMs === "number" && Number.isFinite(data.timeoutMs) && data.timeoutMs > 0,
    "invalid hydration deadline",
  );
  const head = record(data.headDefaults, "headDefaults");
  for (const key of ["title", "description", "canonical", "robots"]) {
    if (Object.hasOwn(head, key)) requireCondition(typeof head[key] === "string", `invalid headDefaults.${key}`);
  }

  const decoder: FrameDecoder = new FrameDecoder(arrayValue(data.frames, "frames"));
  const owners = new Map<string, boolean>();
  for (const raw of arrayValue(data.owners, "owners")) {
    const owner = record(raw, "owner");
    const id = tokenValue(owner.id, "owner id");
    requireCondition(!owners.has(id), `duplicate owner ${id}`);
    requireCondition(typeof owner.retired === "boolean", `invalid owner state ${id}`);
    if (id === "0") requireCondition(!Object.hasOwn(owner, "parentId"), "root owner has a parent");
    else requireCondition(owners.has(tokenValue(owner.parentId, "parent owner")), `missing parent of owner ${id}`);
    owners.set(id, owner.retired as boolean);
  }
  requireCondition(owners.has("0") && owners.get("0") === false, "missing live root owner");

  const routes = new Set<string>();
  for (const raw of arrayValue(data.routes, "routes")) {
    const route = record(raw, "route");
    const id = textValue(route.id, "route id");
    requireCondition(!routes.has(id), `duplicate route ${id}`);
    routes.add(id);
    requireCondition(typeof route.hasData === "boolean", `invalid data presence for route ${id}`);
    const frame = integer(route.frame, `route ${id} frame`);
    decoder.validate(route.params, frame);
    requireCondition(Object.hasOwn(route, "data") === route.hasData, `inconsistent data presence for route ${id}`);
    if (route.hasData) decoder.validate(route.data, frame);
  }

  const resources = new Map<string, { owner: string; length: number }>();
  for (const raw of arrayValue(data.resources, "resources")) {
    const resource = record(raw, "resource");
    const id = tokenValue(resource.id, "resource id");
    const owner = tokenValue(resource.ownerId, `resource ${id} owner`);
    requireCondition(!resources.has(id) && owners.has(owner), `duplicate resource or missing owner ${id}`);
    requireCondition(typeof resource.seeded === "boolean", `invalid resource mode ${id}`);
    const states = arrayValue(resource.states, `resource ${id} states`);
    requireCondition(states.length > 0, `resource ${id} has no initial state`);
    for (const rawState of states) {
      const state = record(rawState, `resource ${id} state`);
      requireCondition(
        typeof state.pending === "boolean" && typeof state.hasResolved === "boolean" && typeof state.hasRejection === "boolean",
        `invalid resource flags ${id}`,
      );
      const frame = integer(state.frame, `resource ${id} frame`);
      decoder.validate({ tag: "undefined" }, frame);
      requireCondition(Object.hasOwn(state, "resolved") === (resource.seeded && state.hasResolved), `inconsistent resolved presence ${id}`);
      requireCondition(Object.hasOwn(state, "rejection") === state.hasRejection, `inconsistent rejection presence ${id}`);
      if (Object.hasOwn(state, "resolved")) decoder.validate(state.resolved, frame);
      if (state.hasRejection) decoder.validate(state.rejection, frame);
    }
    const last = record(states[states.length - 1], `resource ${id} final state`);
    requireCondition(owners.get(owner) || last.pending === false, `live resource ${id} is still pending`);
    resources.set(id, { owner, length: states.length });
  }

  const awaits = new Map<string, AwaitRecord>();
  for (const raw of arrayValue(data.awaitSlots, "awaitSlots")) {
    const slot = record(raw, "await slot");
    const id = tokenValue(slot.id, "await id");
    const owner = tokenValue(slot.ownerId, `await ${id} owner`);
    const occurrence = integer(slot.occurrence, `await ${id} occurrence`);
    const key = awaitKey(id, occurrence);
    requireCondition(!awaits.has(key) && owners.has(owner), `duplicate await or missing owner ${key}`);
    requireCondition(slot.status === "resolved" || slot.status === "rejected" || slot.status === "thrown", `invalid await status ${key}`);
    decoder.validate(slot.value, integer(slot.frame, `await ${key} frame`));
    awaits.set(key, slot as unknown as AwaitRecord);
  }

  const consumedRoutes = new Set<string>();
  const consumedResources = new Map<string, number>();
  const consumedAwaits = new Set<string>();
  const handoffs = arrayValue(data.handoffs, "handoffs");
  for (let seq = 0; seq < handoffs.length; seq += 1) {
    const handoff = record(handoffs[seq], `handoff ${seq}`);
    requireCondition(handoff.seq === seq, `noncontiguous handoff sequence ${seq}`);
    const id = textValue(handoff.id, `handoff ${seq} id`);
    const owner = tokenValue(handoff.ownerId, `handoff ${seq} owner`);
    requireCondition(owners.has(owner), `unknown handoff owner ${owner}`);
    requireCondition(handoff.delivery === "inline" || handoff.delivery === "scheduled", `invalid handoff delivery ${seq}`);
    switch (handoff.kind) {
      case "route":
        requireCondition(routes.has(id) && !consumedRoutes.has(id) && !Object.hasOwn(handoff, "index"), `invalid route handoff ${id}`);
        consumedRoutes.add(id);
        break;
      case "resource": {
        const resource = resources.get(id);
        const index = integer(handoff.index, `resource handoff ${id} index`);
        requireCondition(
          resource !== undefined && resource.owner === owner && index < resource.length && index === (consumedResources.get(id) ?? 0),
          `invalid resource handoff ${id}`,
        );
        consumedResources.set(id, index + 1);
        break;
      }
      case "await": {
        const key = awaitKey(id, integer(handoff.index, `await handoff ${id} occurrence`));
        const input = awaits.get(key);
        requireCondition(
          input?.ownerId === owner && !consumedAwaits.has(key) && handoff.delivery === (input.status === "thrown" ? "inline" : "scheduled"),
          `invalid await handoff ${key}`,
        );
        consumedAwaits.add(key);
        break;
      }
      case "flush":
      case "checkpoint":
        requireCondition(!Object.hasOwn(handoff, "index"), `unexpected handoff index ${seq}`);
        break;
      default:
        throw new HydrationError(`unknown handoff kind at ${seq}`);
    }
  }
  requireCondition(consumedRoutes.size === routes.size && consumedAwaits.size === awaits.size, "missing recorded input handoff");
  for (const [id, resource] of resources) {
    requireCondition(consumedResources.get(id) === resource.length, `missing resource handoff ${id}`);
  }

  const modules = new Set<string>();
  for (const raw of arrayValue(data.modules, "modules")) {
    const id = textValue(raw, "module id");
    requireCondition(!modules.has(id), `duplicate module ${id}`);
    modules.add(id);
  }
  validateLayout(arrayValue(data.layout, "layout"), owners);
  return { payload: data as unknown as HydrationPayload, decoder };
}

export function awaitKey(id: string, occurrence: number): string {
  return `${id}:${occurrence}`;
}

function validateLayout(roots: unknown[], owners: ReadonlyMap<string, boolean>): void {
  const tokens = new Set<string>();
  const queue = [...roots];
  for (let index = 0; index < queue.length; index += 1) {
    const node = record(queue[index], `layout node ${index}`);
    if (Object.hasOwn(node, "index")) integer(node.index, "static node index");
    if (Object.hasOwn(node, "site")) requireCondition(typeof node.site === "string" && SITE_KEY.test(node.site), "invalid layout site key");
    if (Object.hasOwn(node, "token")) {
      const token = tokenValue(node.token, "layout token");
      requireCondition(!tokens.has(token), `duplicate layout token ${token}`);
      tokens.add(token);
    }
    switch (node.kind) {
      case "element":
        textValue(node.tag, "element tag");
        requireCondition(node.ns === "" || node.ns === "svg" || node.ns === "math", "invalid element namespace");
        requireCondition(!Object.hasOwn(node, "opaque") || node.opaque === true, "invalid opaque subtree flag");
        requireCondition(Object.hasOwn(node, "token") === Object.hasOwn(node, "site"), "native root needs both token and site");
        for (const child of arrayValue(node.children, "element children")) queue.push(child);
        break;
      case "range":
        tokenValue(node.token, "range token");
        requireCondition(owners.has(tokenValue(node.ownerId, "range owner")), "unknown range owner");
        requireCondition(RANGE_KINDS.has(node.range), "invalid range kind");
        requireCondition(
          !Object.hasOwn(node, "placement") || node.placement === "body" || node.placement === "inert",
          "invalid portal placement",
        );
        for (const child of arrayValue(node.children, "range children")) queue.push(child);
        break;
      case "text":
        requireCondition(typeof node.text === "string", "invalid layout text");
        break;
      case "marker":
        break;
      default:
        throw new HydrationError(`unknown layout node kind at ${index}`);
    }
  }
}

function record(value: unknown, label: string): Record<string, unknown> {
  requireCondition(typeof value === "object" && value !== null && !Array.isArray(value), `invalid ${label}`);
  return value as Record<string, unknown>;
}

function arrayValue(value: unknown, label: string): unknown[] {
  requireCondition(Array.isArray(value), `invalid ${label}`);
  return value as unknown[];
}

function textValue(value: unknown, label: string): string {
  requireCondition(typeof value === "string" && value.length > 0, `invalid ${label}`);
  return value as string;
}

function tokenValue(value: unknown, label: string): string {
  const token = textValue(value, label);
  requireCondition(TOKEN.test(token), `invalid ${label}`);
  return token;
}

function integer(value: unknown, label: string): number {
  requireCondition(typeof value === "number" && Number.isSafeInteger(value) && value >= 0, `invalid ${label}`);
  return value as number;
}

function requireCondition(condition: unknown, message: string): asserts condition {
  if (!condition) throw new HydrationError(message);
}
