import type { ReactiveNode } from "../graph";
import { getActiveScope, registerNodeScope, scopeOfNode, type ExecutionScope, type SourceSite } from "./scope";

export type { AsyncComputed, AsyncContext } from "../asyncComputed";
export { internalAsyncComputed, resource } from "../asyncComputed";
export { IslandContext, takeSeed, type IslandState, type Seed } from "../island";

export interface ResourceRecord {
  readonly scope: ExecutionScope | undefined;
  readonly pending: Map<number, () => void>;
}

let pendingSite: SourceSite | undefined;
let records: WeakMap<object, ResourceRecord> | undefined;

export function consumePendingSite(): SourceSite | undefined {
  const site = pendingSite;
  pendingSite = undefined;
  return site;
}

export function resourceRecordOf(node: object): ResourceRecord | undefined {
  return records?.get(node);
}

export function registerResource(node: ReactiveNode, owner: ReactiveNode | undefined, isPublic: boolean): void {
  if (isPublic) consumePendingSite();
  const record: ResourceRecord = {
    scope: getActiveScope() ?? (owner !== undefined ? scopeOfNode(owner) : undefined),
    pending: new Map(),
  };
  (records ??= new WeakMap()).set(node, record);
  registerNodeScope(node, owner);
}

function releasePending(record: ResourceRecord, generation: number): void {
  record.pending.get(generation)?.();
  record.pending.delete(generation);
}

export function trackResourceStart(record: ResourceRecord, generation: number, promise: Promise<unknown>): void {
  releasePending(record, generation);
  const scope = record.scope;
  if (scope === undefined || scope.disposed) return;
  record.pending.set(generation, scope.trackPendingWork(promise));
}

export function supersedeResource(record: ResourceRecord, exceptGeneration: number): void {
  for (const generation of record.pending.keys()) {
    if (generation !== exceptGeneration) releasePending(record, generation);
  }
}

export function settleResource(record: ResourceRecord, generation: number, currentGeneration: number): boolean {
  releasePending(record, generation);
  return generation === currentGeneration && record.scope?.disposed !== true;
}

export function disposeResource(node: object): void {
  const record = records?.get(node);
  if (record === undefined) return;
  for (const generation of record.pending.keys()) releasePending(record, generation);
}

export function withResourceSite<T, A extends unknown[]>(site: SourceSite, factory: (...args: A) => T, receiver: unknown, ...args: A): T {
  if (!__REZE_HTML__) return (factory as (this: unknown, ...args: A) => T).apply(receiver, args);
  const prevSite = pendingSite;
  pendingSite = site;
  try {
    return (factory as (this: unknown, ...args: A) => T).apply(receiver, args);
  } finally {
    pendingSite = prevSite;
  }
}
