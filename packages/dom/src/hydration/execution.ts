import { getOwner, onCleanup } from "@rezejs/signals";
import {
  currentModuleId,
  getActiveScope,
  getScopeObserver,
  moduleScopeOf,
  parentOwner,
  scopeOfNode,
  setScopeObserver,
  type ExecutionScope,
  type ReactiveNode,
  type ScopeObserver,
  type SourceSite,
} from "@rezejs/signals/internal/scope";

import { HydrationError, type OwnerRecord } from "./protocol";

export interface ExecutionObserver extends ScopeObserver {
  readonly scope: ExecutionScope;
}

const executions = new WeakMap<ExecutionScope, ExecutionObserver>();
let registrations = 0;
let previousObserver: ScopeObserver | undefined;

function observerFor(scope: ExecutionScope | undefined): ScopeObserver | undefined {
  return (scope === undefined ? undefined : executions.get(scope)) ?? previousObserver;
}

const dispatcher: ScopeObserver = {
  onNodeCreated(node, scope, owner) {
    observerFor(scope)?.onNodeCreated?.(node, scope, owner);
  },
  onNodeDisposed(node) {
    observerFor(scopeOfNode(node))?.onNodeDisposed?.(node);
  },
  onFlushBoundary(scope, delivery, phase) {
    observerFor(scope)?.onFlushBoundary?.(scope, delivery, phase);
  },
  onScopeWork(scope) {
    observerFor(scope)?.onScopeWork?.(scope);
  },
  onResourceRegistered(resource) {
    observerFor(resource.scope)?.onResourceRegistered?.(resource);
  },
  shouldSkipInitialProducer(resource) {
    return observerFor(resource.scope)?.shouldSkipInitialProducer?.(resource) ?? false;
  },
  onResourceState(resource, snapshot) {
    observerFor(resource.scope)?.onResourceState?.(resource, snapshot);
  },
  shouldHoldSettlement(resource) {
    return observerFor(resource.scope)?.shouldHoldSettlement?.(resource) ?? false;
  },
  onModuleAwait(frame, site) {
    observerFor(frame.scope)?.onModuleAwait?.(frame, site);
  },
  interceptModuleAwait(frame, value) {
    return observerFor(frame.scope)?.interceptModuleAwait?.(frame, value);
  },
  onModuleResume(frame) {
    observerFor(frame.scope)?.onModuleResume?.(frame);
  },
  onContinuationEvent(handle, event, site, value) {
    observerFor(handle.scope)?.onContinuationEvent?.(handle, event, site, value);
  },
  interceptSuspend(handle, site, value) {
    return observerFor(handle.scope)?.interceptSuspend?.(handle, site, value);
  },
  interceptContinuationValue(handle, event, value) {
    return observerFor(handle.scope)?.interceptContinuationValue?.(handle, event, value);
  },
  resolveUniqueId(request) {
    return observerFor(request.scope)?.resolveUniqueId?.(request);
  },
};

export function registerExecution(execution: ExecutionObserver): () => void {
  if (executions.has(execution.scope)) throw new HydrationError("execution scope already has a session");
  if (registrations === 0) {
    previousObserver = getScopeObserver();
    setScopeObserver(dispatcher);
  }
  registrations += 1;
  executions.set(execution.scope, execution);
  let registered = true;
  return () => {
    if (!registered) return;
    registered = false;
    executions.delete(execution.scope);
    registrations -= 1;
    if (registrations === 0) {
      if (getScopeObserver() === dispatcher) setScopeObserver(previousObserver);
      previousObserver = undefined;
    }
  };
}

export function currentExecution(): ExecutionObserver | undefined {
  const owner = getOwner();
  const scope = getActiveScope() ?? (owner === undefined ? undefined : scopeOfNode(owner));
  return scope === undefined ? undefined : executions.get(scope);
}

export function moduleExecution(moduleId: string): ExecutionObserver | undefined {
  const scope = moduleScopeOf(moduleId);
  return scope === undefined ? undefined : executions.get(scope);
}

export class Instance {
  readonly counters = new Map<string, number>();
  readonly children: Instance[] = [];
  registered = false;
  retired = false;

  constructor(
    readonly id: string,
    readonly parent: Instance | undefined,
    readonly site: SourceSite | undefined,
    readonly moduleId?: string,
  ) {}

  next(key: string): number {
    const next = this.counters.get(key) ?? 0;
    this.counters.set(key, next + 1);
    return next;
  }
}

export class Instances {
  readonly root = new Instance("0", undefined, undefined);
  readonly modules = new Set<string>();
  private readonly owners = new WeakMap<ReactiveNode, Instance>();
  private readonly moduleOwners = new Map<string, Instance>();
  private readonly records: Instance[] = [];
  private readonly registeredIds = new Set<string>();
  private active: Instance | undefined;
  private activeOwner: ReactiveNode | undefined;
  private readonly previousOwners: (ReactiveNode | undefined)[] = [];

  constructor() {
    this.use(this.root);
  }

  private use(instance: Instance): Instance {
    if (!instance.registered) {
      if (instance.parent !== undefined) this.use(instance.parent);
      instance.registered = true;
      this.records.push(instance);
      this.registeredIds.add(instance.id);
    }
    return instance;
  }

  module(moduleId: string): Instance {
    let instance = this.moduleOwners.get(moduleId);
    if (instance === undefined) {
      let token = "0.m";
      for (let index = 0; index < moduleId.length; index += 1) {
        token += moduleId.charCodeAt(index).toString(16).padStart(4, "0");
      }
      instance = new Instance(token, this.root, undefined, moduleId);
      this.root.children.push(instance);
      this.moduleOwners.set(moduleId, instance);
    }
    return instance;
  }

  bind(owner: ReactiveNode, instance: Instance): void {
    this.owners.set(owner, instance);
  }

  inherit(owner: ReactiveNode, parent: ReactiveNode | undefined): void {
    if (!this.owners.has(owner)) this.owners.set(owner, this.current(parent));
  }

  current(owner: ReactiveNode | undefined = getOwner(), moduleId: string | undefined = currentModuleId()): Instance {
    if (this.active !== undefined && owner === this.activeOwner) return this.use(this.active);
    let node = owner;
    while (node !== undefined) {
      const instance = this.owners.get(node);
      if (instance !== undefined) return this.use(instance);
      node = parentOwner(node);
    }
    return this.use(moduleId === undefined ? this.root : this.module(moduleId));
  }

  reserve(kind: string, site?: SourceSite, parent: Instance = this.current()): Instance {
    if (parent.retired) throw new HydrationError("work created under a retired owner", site);
    if (site !== undefined) this.modules.add(site.module);
    if (parent.moduleId !== undefined) this.modules.add(parent.moduleId);
    const key = kind + (site?.key ?? "");
    const instance = new Instance(`${parent.id}.${key}_${parent.next(key).toString(36)}`, parent, site);
    parent.children.push(instance);
    return this.use(instance);
  }

  own(instance: Instance): void {
    if (getOwner() !== undefined) onCleanup(() => this.retire(instance));
  }

  enter(instance: Instance): Instance | undefined {
    const previous = this.active;
    this.previousOwners.push(this.activeOwner);
    this.activeOwner = getOwner();
    this.active = instance;
    return previous;
  }

  restore(previous: Instance | undefined): void {
    this.active = previous;
    this.activeOwner = this.previousOwners.pop();
  }

  run<T>(instance: Instance, fn: () => T): T {
    const previous = this.enter(instance);
    try {
      return fn();
    } finally {
      this.restore(previous);
    }
  }

  retire(instance: Instance): void {
    if (instance.retired) return;
    const pending = [instance];
    while (pending.length !== 0) {
      const current = pending.pop()!;
      current.retired = true;
      for (const child of current.children) if (!child.retired) pending.push(child);
    }
  }

  uniqueId(owner: ReactiveNode | undefined, site?: SourceSite, moduleId?: string): string {
    const instance = this.use(this.current(owner, moduleId));
    if (site !== undefined) this.modules.add(site.module);
    if (moduleId !== undefined) this.modules.add(moduleId);
    const key = `u${site?.key ?? ""}`;
    const token = `${instance.id}.${key}_${instance.next(key).toString(36)}`;
    return `rz${token.replaceAll("_", "__").replaceAll(".", "_d")}`;
  }

  has(id: string): boolean {
    return this.registeredIds.has(id);
  }

  release(): void {
    for (const instance of this.records) {
      instance.counters.clear();
      instance.children.length = 0;
    }
    this.records.length = 0;
    this.registeredIds.clear();
    this.moduleOwners.clear();
    this.modules.clear();
    this.active = undefined;
    this.activeOwner = undefined;
    this.previousOwners.length = 0;
  }

  snapshot(): OwnerRecord[] {
    return this.records.map((instance) => instance.parent === undefined
      ? { id: instance.id, retired: instance.retired }
      : { id: instance.id, parentId: instance.parent.id, retired: instance.retired });
  }
}
