import { getOwner, onCleanup, root, runWithOwner } from "@rezejs/signals";
import type { ContinuationEvent, ContinuationHandle } from "@rezejs/signals/internal/continuation";
import type { ResourceRecord as RuntimeResource, ResourceSnapshot } from "@rezejs/signals/internal/resource";
import {
  createScope,
  registerModuleScope,
  ScopeTimeoutError,
  type ExecutionScope,
  type FlushDelivery,
  type ReactiveNode,
  type SourceSite,
} from "@rezejs/signals/internal/scope";
import type { ModuleScopeFrame } from "@rezejs/signals/internal/scope";

import { FrameEncoder } from "../hydration/codec";
import { Instances, registerExecution, type ExecutionObserver, type Instance } from "../hydration/execution";
import {
  HydrationError,
  type AwaitRecord,
  type Handoff,
  type HeadDefaults,
  type HydrationPayload,
  type LayoutNode,
  type ResourceRecord,
  type ResourceStateRecord,
  type RouteRecord,
} from "../hydration/protocol";
import type { HtmlLayoutRangeInfo } from "./serialize";
import type { HtmlRange } from "./tree";
export interface HtmlSessionOptions {
  readonly pathname: string;
  readonly rootId: string;
  readonly buildId: string;
  readonly timeoutMs?: number;
  readonly headDefaults?: HeadDefaults;
  readonly modules?: readonly string[];
}

interface RecordedResource {
  readonly instance: Instance;
  readonly seeded: boolean;
  readonly states: ResourceStateRecord[];
}

interface RecordedContinuation {
  readonly instance: Instance;
  previous: Instance | undefined;
  entered: boolean;
  operand?: SourceSite;
  outerOperands?: SourceSite[];
  suspended?: { id: string; occurrence: number; release: () => void };
}

export class HtmlSession implements ExecutionObserver {
  readonly scope: ExecutionScope;
  readonly instances = new Instances();
  readonly encoder: FrameEncoder;
  private readonly owner: ReactiveNode;
  private readonly unregister: () => void;
  private readonly deadline: number;
  readonly ranges = new Map<string, HtmlLayoutRangeInfo>();
  readonly portals: { node: HtmlRange; instance: Instance; placement: "body" | "inert" }[] = [];
  private readonly timeoutMs: number;
  private readonly resources = new Map<ReactiveNode, RecordedResource>();
  private readonly continuations = new WeakMap<ContinuationHandle, RecordedContinuation>();
  private moduleAwaits: WeakMap<ModuleScopeFrame, Instance> | undefined;
  private readonly routes: RouteRecord[] = [];
  private readonly awaits: AwaitRecord[] = [];
  private readonly handoffs: Handoff[] = [];
  private readonly failure = Promise.withResolvers<never>();
  private failed = false;
  private error: unknown;
  private disposed = false;

  constructor(private readonly options: HtmlSessionOptions) {
    this.timeoutMs = options.timeoutMs ?? 30_000;
    this.scope = createScope({ name: options.pathname, controlled: true, timeoutMs: this.timeoutMs });
    this.deadline = Date.now() + this.timeoutMs;
    this.encoder = new FrameEncoder(options.pathname);
    this.unregister = registerExecution(this);
    this.owner = this.scope.run(() => root(() => getOwner()!));
    this.instances.bind(this.owner, this.instances.root);
    this.failure.promise.catch(() => {});
    const createOwner = (moduleId: string): ReactiveNode =>
      this.run(() =>
        root(() => {
          const owner = getOwner()!;
          this.instances.bind(owner, this.instances.module(moduleId));
          this.instances.modules.add(moduleId);
          return owner;
        }),
      );
    for (const moduleId of options.modules ?? []) registerModuleScope(moduleId, this.scope, undefined, createOwner);
  }

  run<T>(fn: () => T): T {
    this.check();
    return this.scope.run(() => runWithOwner(this.owner, fn));
  }

  async load<T>(loader: () => Promise<T>): Promise<T> {
    const loaded = Promise.resolve(this.run(loader)).catch((error: unknown) => {
      this.fail(error);
      throw error;
    });
    this.scope.trackPendingWork(loaded);
    await this.settle();
    return loaded;
  }

  async settle(): Promise<void> {
    this.check();
    const remaining = this.deadline - Date.now();
    if (remaining <= 0) throw new ScopeTimeoutError(this.options.pathname, this.timeoutMs);
    await Promise.race([this.scope.settle(remaining), this.failure.promise]);
    this.check();
  }

  recordRoute(id: string, params: unknown, hasData: boolean, data?: unknown): void {
    this.check();
    const frame = this.encoder.capture(`route:${id}`, hasData ? [params, data] : [params]);
    this.routes.push({ id, params: frame.values[0]!, hasData, ...(hasData ? { data: frame.values[1]! } : {}), frame: frame.frame });
    this.record("route", id, this.instances.current().id, "inline");
  }

  snapshot(layout: readonly LayoutNode[]): HydrationPayload {
    this.check();
    const resources: ResourceRecord[] = [];
    for (const resource of this.resources.values()) {
      resources.push({ id: resource.instance.id, ownerId: resource.instance.id, seeded: resource.seeded, states: resource.states.slice() });
    }
    return {
      version: 1,
      buildId: this.options.buildId,
      rootId: this.options.rootId,
      pathname: this.options.pathname,
      timeoutMs: this.timeoutMs,
      headDefaults: { ...this.options.headDefaults },
      routes: this.routes.slice(),
      resources,
      awaitSlots: this.awaits.slice(),
      frames: this.encoder.frames.slice(),
      owners: this.instances.snapshot(),
      handoffs: this.handoffs.slice(),
      modules: [...this.instances.modules],
      layout,
    };
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    try {
      this.scope.dispose();
    } finally {
      this.unregister();
    }
  }

  onNodeCreated(node: ReactiveNode, _scope: ExecutionScope | undefined, owner: ReactiveNode | undefined): void {
    this.instances.inherit(node, owner);
  }

  onNodeDisposed(node: ReactiveNode): void {
    const resource = this.resources.get(node);
    if (resource !== undefined) this.instances.retire(resource.instance);
  }

  onResourceRegistered(resource: RuntimeResource): void {
    this.guard(() => {
      const instance = this.instances.reserve("r", resource.site, this.instances.current(resource.owner, resource.moduleId));
      this.instances.bind(resource.node, instance);
      this.resources.set(resource.node, { instance, seeded: resource.kind === "public", states: [] });
    });
  }

  onResourceState(resource: RuntimeResource, state: ResourceSnapshot): void {
    this.guard(() => {
      const recorded = this.resources.get(resource.node);
      if (recorded === undefined) throw new HydrationError("resource was not registered", resource.site);
      const values: unknown[] = [];
      if (recorded.seeded && state.hasResolved) values.push(state.resolved);
      if (state.hasRejection) values.push(state.rejection);
      const frame = this.encoder.capture(recorded.instance.id, values);
      let index = 0;
      const snapshot: ResourceStateRecord = {
        pending: state.pending,
        hasResolved: state.hasResolved,
        hasRejection: state.hasRejection,
        ...(recorded.seeded && state.hasResolved ? { resolved: frame.values[index++]! } : {}),
        ...(state.hasRejection ? { rejection: frame.values[index]! } : {}),
        frame: frame.frame,
      };
      const occurrence = recorded.states.length;
      recorded.states.push(snapshot);
      this.record("resource", recorded.instance.id, recorded.instance.id, state.pending ? "inline" : "scheduled", occurrence);
    });
  }

  onFlushBoundary(_scope: ExecutionScope, delivery: FlushDelivery, phase: "before" | "after"): void {
    if (!this.failed && !this.disposed)
      this.record(phase === "before" ? "flush" : "checkpoint", `flush.${phase}`, this.instances.current().id, delivery);
  }

  onModuleAwait(frame: ModuleScopeFrame, site: SourceSite): void {
    this.guard(() => {
      const instance = this.instances.reserve("n", site, this.instances.current(frame.moduleOwner, frame.moduleId));
      (this.moduleAwaits ??= new WeakMap()).set(frame, instance);
      this.record("checkpoint", `${instance.id}.suspend`, instance.id, "inline");
    });
  }

  onModuleResume(frame: ModuleScopeFrame): void {
    this.guard(() => {
      const instance = this.moduleAwaits?.get(frame);
      if (instance === undefined) throw new HydrationError("module await was not registered");
      this.moduleAwaits?.delete(frame);
      this.record("checkpoint", `${instance.id}.resume`, instance.id, "scheduled");
    });
  }

  beginOperand(handle: ContinuationHandle, site: SourceSite): void {
    this.check();
    const record = this.continuations.get(handle);
    if (record === undefined) throw new HydrationError("continuation was not registered", site);
    if (record.operand !== undefined) (record.outerOperands ??= []).push(record.operand);
    record.operand = site;
  }

  rejectOperand(handle: ContinuationHandle, value: unknown): void {
    this.check();
    const record = this.continuations.get(handle);
    if (record === undefined) throw new HydrationError("continuation was not registered", handle.site);
    const site = record.operand;
    if (site === undefined) return;
    record.operand = undefined;
    if (record.outerOperands !== undefined) record.outerOperands.length = 0;
    const id = `${record.instance.id}.a${site.key}`;
    const occurrence = record.instance.next(`a${site.key}`);
    const frame = this.encoder.capture(id, [value]);
    this.awaits.push({ id, ownerId: record.instance.id, occurrence, status: "thrown", value: frame.values[0]!, frame: frame.frame });
    this.record("await", id, record.instance.id, "inline", occurrence);
  }

  onContinuationEvent(handle: ContinuationHandle, event: ContinuationEvent, site: SourceSite | undefined, value: unknown): void {
    this.guard(() => {
      if (event === "begin") {
        const instance = this.instances.reserve("c", site, this.instances.current(handle.owner, handle.moduleId));
        const record: RecordedContinuation = { instance, previous: this.instances.enter(instance), entered: true };
        this.continuations.set(handle, record);
        onCleanup(() => {
          record.suspended?.release();
          this.instances.retire(instance);
        });
        this.record("checkpoint", `${instance.id}.begin`, instance.id, "inline");
        return;
      }
      const record = this.continuations.get(handle);
      if (record === undefined) throw new HydrationError("continuation was not registered", site);
      if (event === "resume" || event === "reject") {
        const suspended = record.suspended;
        if (suspended === undefined) throw new HydrationError("continuation resumed without an await slot", site);
        record.suspended = undefined;
        suspended.release();
        record.previous = this.instances.enter(record.instance);
        record.entered = true;
        if (event === "reject") {
          record.operand = undefined;
          if (record.outerOperands !== undefined) record.outerOperands.length = 0;
        }
        const frame = this.encoder.capture(suspended.id, [value]);
        this.awaits.push({
          id: suspended.id,
          ownerId: record.instance.id,
          occurrence: suspended.occurrence,
          status: event === "resume" ? "resolved" : "rejected",
          value: frame.values[0]!,
          frame: frame.frame,
        });
        this.record("await", suspended.id, record.instance.id, "scheduled", suspended.occurrence);
        return;
      }
      if (event === "suspend") {
        if (site === undefined) throw new HydrationError("managed await is missing a compiler site");
        record.operand = record.outerOperands?.pop();
        const id = `${record.instance.id}.a${site.key}`;
        const occurrence = record.instance.next(`a${site.key}`);
        const gate = Promise.withResolvers<void>();
        const cancel = this.scope.trackPendingWork(gate.promise);
        record.suspended = {
          id,
          occurrence,
          release: () => {
            cancel();
            gate.resolve();
          },
        };
        this.record("checkpoint", `${id}_${occurrence.toString(36)}.suspend`, record.instance.id, "inline");
      } else {
        record.suspended?.release();
        record.suspended = undefined;
        this.record("checkpoint", `${record.instance.id}.end`, record.instance.id, "inline");
      }
      if (record.entered) {
        this.instances.restore(record.previous);
        record.previous = undefined;
        record.entered = false;
      }
    });
  }

  resolveUniqueId(request: { owner: ReactiveNode | undefined; site?: SourceSite; moduleId: string | undefined }): string {
    return this.instances.uniqueId(request.owner, request.site, request.moduleId);
  }

  private record(kind: Handoff["kind"], id: string, ownerId: string, delivery: FlushDelivery, index?: number): void {
    this.handoffs.push({ seq: this.handoffs.length, kind, id, ownerId, delivery, ...(index === undefined ? {} : { index }) });
  }

  private guard(fn: () => void): void {
    if (this.failed || this.disposed) return;
    try {
      fn();
    } catch (error) {
      this.fail(error);
    }
  }

  private fail(error: unknown): void {
    if (this.failed) return;
    this.failed = true;
    this.error = error;
    this.failure.reject(error);
  }

  private check(): void {
    if (this.failed) throw this.error;
    if (this.disposed) throw new HydrationError("HTML session is disposed");
  }
}
