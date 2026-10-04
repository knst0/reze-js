import { getOwner } from "../context";
import type { ReactiveNode } from "../graph";
import {
  currentModuleId,
  enterScopeContext,
  getActiveScope,
  getScopeObserver,
  registerNodeScope,
  restoreScopeContext,
  scopeOfNode,
  withModuleScope,
  type ExecutionScope,
  type SourceSite,
} from "./scope";

export type { AsyncComputed, AsyncContext } from "../asyncComputed";
export { internalAsyncComputed } from "../asyncComputed";

export type ResourceKind = "public" | "internal";

export interface ResourceSnapshot {
  readonly pending: boolean;
  readonly hasResolved: boolean;
  readonly resolved?: unknown;
  readonly hasRejection: boolean;
  readonly rejection?: unknown;
}

export interface ResourceRecord {
  readonly node: ReactiveNode;
  readonly owner: ReactiveNode | undefined;
  readonly scope: ExecutionScope | undefined;
  readonly site: SourceSite | undefined;
  readonly moduleId: string | undefined;
  readonly id: string;
  readonly kind: ResourceKind;
  committed: ResourceSnapshot;
  activated: boolean;
  held: boolean;
  hasResolved: boolean;
  hasRejection: boolean;
}

export interface AppliedResourceState {
  readonly pending: boolean;
  readonly hasResolved: boolean;
  readonly resolved?: unknown;
  readonly hasRejection: boolean;
  readonly rejection?: unknown;
}

export interface ResourceController {
  hold(): void;
  release(): void;
  applyState(state: AppliedResourceState): void;
  activate(): void;
  cancel(): void;
}

export interface ManagedResourceNode extends ReactiveNode {
  generation: number;
  hasSettled: boolean;
  start(): void;
}

interface ResourceIO {
  writeResolved(value: unknown): void;
  writeRejection(value: unknown): void;
  writePending(value: boolean): void;
  purge(): void;
}

interface HeldSettlement {
  generation: number;
  snapshot: ResourceSnapshot;
}

let pendingSite: SourceSite | undefined;
let records: WeakMap<object, ResourceRecord> | undefined;
let ioByNode: WeakMap<object, ResourceIO> | undefined;
let pendingByRecord: WeakMap<ResourceRecord, Map<number, () => void>> | undefined;
let heldByRecord: WeakMap<ResourceRecord, HeldSettlement> | undefined;
let occurrences: WeakMap<ExecutionScope, number> | undefined;
let anonymousOccurrences = 0;
let skipResolvers: WeakMap<ResourceRecord, () => void> | undefined;

export function consumePendingSite(): SourceSite | undefined {
  const site = pendingSite;
  pendingSite = undefined;
  return site;
}

export function resourceRecordOf(node: object): ResourceRecord | undefined {
  return records?.get(node);
}

function occurrenceId(site: SourceSite): string {
  return `${site.module}:${site.key}#${site.ordinal.toString(36)}`;
}

function anonymousId(scope: ExecutionScope | undefined): string {
  if (scope === undefined) {
    anonymousOccurrences += 1;
    return `anonymous:${anonymousOccurrences.toString(36)}`;
  }
  if (occurrences === undefined) {
    occurrences = new WeakMap();
  }
  const next = (occurrences.get(scope) ?? 0) + 1;
  occurrences.set(scope, next);
  return `scope:${next.toString(36)}`;
}

const INITIAL_COMMITTED: ResourceSnapshot = {
  pending: true,
  hasResolved: false,
  resolved: undefined,
  hasRejection: false,
  rejection: undefined,
};

export function registerResource(
  node: ManagedResourceNode,
  owner: ReactiveNode | undefined,
  kind: ResourceKind,
  io: ResourceIO,
): ResourceRecord {
  const site = kind === "public" ? consumePendingSite() : undefined;
  const scope = getActiveScope() ?? (owner !== undefined ? scopeOfNode(owner) : undefined);
  const record: ResourceRecord = {
    node,
    owner,
    scope,
    site,
    moduleId: currentModuleId() ?? site?.module,
    id: site !== undefined ? occurrenceId(site) : anonymousId(scope),
    kind,
    committed: INITIAL_COMMITTED,
    activated: false,
    held: false,
    hasResolved: false,
    hasRejection: false,
  };
  (records ??= new WeakMap()).set(node, record);
  (ioByNode ??= new WeakMap()).set(node, io);
  registerNodeScope(node, owner);
  getScopeObserver()?.onResourceRegistered?.(record);
  return record;
}

function releasePending(record: ResourceRecord, generation: number): void {
  const entries = pendingByRecord?.get(record);
  entries?.get(generation)?.();
  entries?.delete(generation);
}

function trackPending(record: ResourceRecord, generation: number, promise: Promise<unknown>): void {
  releasePending(record, generation);
  let entries = pendingByRecord?.get(record);
  if (entries === undefined) {
    entries = new Map();
    (pendingByRecord ??= new WeakMap()).set(record, entries);
  }
  const scope = record.scope;
  if (scope === undefined || scope.disposed) {
    return;
  }
  entries.set(generation, scope.trackPendingWork(promise));
}

export function trackResourceStart(record: ResourceRecord, generation: number, promise: Promise<unknown>): void {
  trackPending(record, generation, promise);
}

export function trackResourceGate(record: ResourceRecord, generation: number, gate: Promise<unknown>): void {
  trackPending(record, generation, gate);
}

export function parkSkippedProducer(record: ResourceRecord): void {
  const { promise, resolve } = Promise.withResolvers<unknown>();
  (skipResolvers ??= new WeakMap()).set(record, () => resolve(undefined));
  trackResourceGate(record, (record.node as ManagedResourceNode).generation, promise);
}

function releaseSkipGate(record: ResourceRecord): void {
  skipResolvers?.get(record)?.();
  skipResolvers?.delete(record);
}

function pendingSnapshot(record: ResourceRecord): ResourceSnapshot {
  const committed = record.committed;
  return {
    pending: true,
    hasResolved: committed.hasResolved,
    resolved: committed.resolved,
    hasRejection: committed.hasRejection,
    rejection: committed.rejection,
  };
}

export function beforeResourcePending(record: ResourceRecord): void {
  const pending = pendingSnapshot(record);
  getScopeObserver()?.onResourceState?.(record, pending);
  record.committed = pending;
}

function writeSettlement(record: ResourceRecord, snapshot: ResourceSnapshot): void {
  if (record.scope?.disposed) return;
  const previous = enterScopeContext(record.scope, record.moduleId);
  try {
    releaseSkipGate(record);
    record.committed = snapshot;
    record.hasResolved = snapshot.hasResolved;
    record.hasRejection = snapshot.hasRejection;
    if (snapshot.hasResolved || snapshot.hasRejection) {
      (record.node as ManagedResourceNode).hasSettled = true;
    }
    const io = ioByNode?.get(record.node);
    if (io === undefined) return;
    if (!snapshot.pending) io.purge();
    if (snapshot.hasResolved) io.writeResolved(snapshot.resolved);
    io.writeRejection(snapshot.hasRejection ? snapshot.rejection : undefined);
    io.writePending(snapshot.pending);
  } finally {
    restoreScopeContext(previous);
  }
}

export function beforeResourceSettle(
  record: ResourceRecord,
  generation: number,
  snapshot: ResourceSnapshot,
): boolean {
  const node = record.node as ManagedResourceNode;
  if (generation !== node.generation || record.scope?.disposed === true) {
    releasePending(record, generation);
    return false;
  }
  releasePending(record, generation);
  const committed = record.committed;
  const full: ResourceSnapshot = snapshot.hasResolved
    ? {
        pending: false,
        hasResolved: true,
        resolved: snapshot.resolved,
        hasRejection: false,
        rejection: undefined,
      }
    : {
        pending: false,
        hasResolved: committed.hasResolved,
        resolved: committed.resolved,
        hasRejection: true,
        rejection: snapshot.rejection,
      };
  getScopeObserver()?.onResourceState?.(record, full);
  if (generation !== node.generation || record.scope?.disposed) return false;
  if (record.held || getScopeObserver()?.shouldHoldSettlement?.(record) === true) {
    record.held = true;
    (heldByRecord ??= new WeakMap()).set(record, { generation, snapshot: full });
    return false;
  }
  record.committed = full;
  record.hasResolved = full.hasResolved;
  record.hasRejection = full.hasRejection;
  return true;
}

export function supersedeResource(record: ResourceRecord, exceptGeneration: number): void {
  releaseSkipGate(record);
  const entries = pendingByRecord?.get(record);
  if (entries !== undefined) {
    for (const generation of [...entries.keys()]) {
      if (generation !== exceptGeneration) {
        releasePending(record, generation);
      }
    }
  }
  heldByRecord?.delete(record);
}

export function disposeResource(node: object): void {
  const record = records?.get(node);
  if (record === undefined) {
    return;
  }
  releaseSkipGate(record);
  const entries = pendingByRecord?.get(record);
  if (entries !== undefined) {
    for (const generation of [...entries.keys()]) {
      releasePending(record, generation);
    }
    pendingByRecord?.delete(record);
  }
  heldByRecord?.delete(record);
}

export function controlResource(node: object): ResourceController | undefined {
  const record = records?.get(node);
  if (record === undefined) {
    return undefined;
  }
  return {
    hold(): void {
      record.held = true;
    },
    release(): void {
      record.held = false;
      const held = heldByRecord?.get(record);
      heldByRecord?.delete(record);
      if (held !== undefined && held.generation === (record.node as ManagedResourceNode).generation) {
        writeSettlement(record, held.snapshot);
      }
    },
    applyState(state: AppliedResourceState): void {
      heldByRecord?.delete(record);
      writeSettlement(record, state);
    },
    activate(): void {
      if (record.activated || record.scope?.disposed) return;
      record.activated = true;
      record.held = false;
      heldByRecord?.delete(record);
      (record.node as ManagedResourceNode).start();
    },
    cancel(): void {
      disposeResource(node);
    },
  };
}

export function withResourceSite<T, A extends unknown[]>(
  site: SourceSite,
  factory: (...args: A) => T,
  receiver: unknown,
  ...args: A
): T {
  if (!(__REZE_HTML__ || __REZE_HYDRATE__)) {
    return (factory as (this: unknown, ...args: A) => T).apply(receiver, args);
  }
  const prevSite = pendingSite;
  pendingSite = site;
  const orphan = getOwner() === undefined && getActiveScope() === undefined;
  if (!orphan) {
    try {
      return (factory as (this: unknown, ...args: A) => T).apply(receiver, args);
    } finally {
      pendingSite = prevSite;
    }
  }
  try {
    return withModuleScope(site.module, () =>
      (factory as (this: unknown, ...args: A) => T).apply(receiver, args),
    );
  } finally {
    pendingSite = prevSite;
  }
}
