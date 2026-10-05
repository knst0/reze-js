import { getOwner, onCleanup, root, runWithOwner } from "@rezejs/signals";
import type { ContinuationEvent, ContinuationHandle } from "@rezejs/signals/internal/continuation";
import {
  controlResource,
  type ResourceController,
  type ResourceRecord as RuntimeResource,
  type ResourceSnapshot,
} from "@rezejs/signals/internal/resource";
import {
  createScope,
  registerModuleScope,
  type ExecutionScope,
  type FlushDelivery,
  type ReactiveNode,
  type SourceSite,
} from "@rezejs/signals/internal/scope";
import type { ModuleScopeFrame } from "@rezejs/signals/internal/scope";

import type { FrameDecoder } from "./codec";
import { Instances, registerExecution, type ExecutionObserver, type Instance } from "./execution";
import {
  awaitKey,
  HydrationError,
  type AwaitRecord,
  type Handoff,
  type HeadDefaults,
  type HydrationPayload,
  type ResourceRecord,
  type ResourceStateRecord,
  type RouteRecord,
} from "./protocol";

interface ResourceConsumer {
  readonly instance: Instance;
  readonly record: ResourceRecord;
  readonly runtime: RuntimeResource;
  readonly controller: ResourceController;
  readonly ready: ResourceSnapshot[];
  index: number;
}

interface AwaitConsumer {
  readonly id: string;
  readonly occurrence: number;
  readonly instance: Instance;
  readonly promise: Promise<unknown>;
  readonly resolve: (value: unknown) => void;
  readonly reject: (error: unknown) => void;
  readonly cancel: () => void;
  released: boolean;
}

interface ContinuationConsumer {
  readonly instance: Instance;
  previous: Instance | undefined;
  entered: boolean;
  pending: AwaitConsumer | undefined;
}

interface ModuleAwaitConsumer {
  readonly instance: Instance;
  readonly promise: Promise<unknown>;
  readonly resolve: (value: unknown) => void;
  readonly reject: (error: unknown) => void;
  readonly cancel: () => void;
  ready: boolean;
  rejected: boolean;
  released: boolean;
  value: unknown;
}

export class HydrationReplay implements ExecutionObserver {
  readonly scope: ExecutionScope;
  readonly instances = new Instances();
  readonly headDefaults: HeadDefaults;
  private readonly owner: ReactiveNode;
  private readonly unregister: () => void;
  private data: HydrationPayload | undefined;
  private decoder: FrameDecoder | undefined;
  private readonly resourceInputs = new Map<string, ResourceRecord>();
  private readonly routeInputs = new Map<string, RouteRecord>();
  private readonly awaitInputs = new Map<string, AwaitRecord>();
  private readonly resources = new Map<ReactiveNode, ResourceConsumer>();
  private readonly resourcesById = new Map<string, ResourceConsumer>();
  private readonly continuations = new WeakMap<ContinuationHandle, ContinuationConsumer>();
  private readonly awaits = new Map<string, AwaitConsumer>();
  private moduleAwaits: WeakMap<ModuleScopeFrame, ModuleAwaitConsumer> | undefined;
  private moduleAwaitsById: Map<string, ModuleAwaitConsumer> | undefined;
  private readonly retired = new Set<string>();
  private readonly failure = Promise.withResolvers<never>();
  private changed: PromiseWithResolvers<void> | undefined;
  private progress = 0;
  private readonly deadline: number;
  private timer: number | undefined;
  private sequence = 0;
  private failed = false;
  private error: unknown;
  private disposed = false;
  private active = true;
  private activated = false;
  private finalizing = false;
  private driving = false;
  private viewStarted = false;

  constructor(payload: HydrationPayload, decoder: FrameDecoder) {
    this.data = payload;
    this.decoder = decoder;
    this.headDefaults = { ...payload.headDefaults };
    this.deadline = Date.now() + payload.timeoutMs;
    this.scope = createScope({ name: payload.pathname, controlled: true, timeoutMs: payload.timeoutMs });
    for (const resource of payload.resources) this.resourceInputs.set(resource.id, resource);
    for (const route of payload.routes) this.routeInputs.set(route.id, route);
    for (const slot of payload.awaitSlots) this.awaitInputs.set(awaitKey(slot.id, slot.occurrence), slot);
    for (const owner of payload.owners) if (owner.retired) this.retired.add(owner.id);
    this.unregister = registerExecution(this);
    this.owner = this.scope.run(() => root(() => getOwner()!));
    this.instances.bind(this.owner, this.instances.root);
    const createOwner = (moduleId: string): ReactiveNode =>
      this.run(() =>
        root(() => {
          const owner = getOwner()!;
          this.instances.bind(owner, this.instances.module(moduleId));
          this.instances.modules.add(moduleId);
          return owner;
        }),
      );
    for (const moduleId of payload.modules) registerModuleScope(moduleId, this.scope, undefined, createOwner);
    this.failure.promise.catch(() => {});
    this.timer = window.setTimeout(() => this.armDeadline(), Math.max(0, Math.min(this.deadline - Date.now(), 2_147_483_647)));
  }

  get preparing(): boolean {
    return this.active && !this.disposed;
  }

  run<T>(fn: () => T): T {
    this.check();
    return this.scope.run(() => runWithOwner(this.owner, fn));
  }

  beginView(): void {
    this.check();
    this.viewStarted = true;
  }

  async load<T>(loader: () => Promise<T>): Promise<T> {
    this.check();
    let complete = false;
    const loaded = Promise.resolve(this.run(loader)).then(
      (value) => {
        complete = true;
        this.wake();
        return value;
      },
      (error: unknown) => {
        this.fail(error);
        throw error;
      },
    );
    this.scope.trackPendingWork(loaded);
    await this.drive(() => complete, false);
    return loaded;
  }

  readRoute(id: string): { params: unknown; hasData: boolean; data?: unknown } {
    this.check();
    const input = this.routeInputs.get(id);
    if (input === undefined) throw this.mismatch(`missing route input ${id}`);
    this.consume("route", id, "0", "inline");
    this.decoder!.apply(input.frame);
    this.routeInputs.delete(id);
    return {
      params: this.decoder!.read(input.params),
      hasData: input.hasData,
      ...(input.hasData ? { data: this.decoder!.read(input.data!) } : {}),
    };
  }

  async settle(): Promise<void> {
    await this.drive(() => this.sequence === this.data!.handoffs.length, true);
  }

  protected beginCommit(): void {
    this.check();
    if (!this.active) return;
    this.active = false;
    this.unregister();
    this.scope.activate();
  }

  activate(): void {
    this.check();
    if (this.activated) return;
    this.activated = true;
    this.beginCommit();
    window.clearTimeout(this.timer);
    this.timer = undefined;
    try {
      for (const resource of this.resources.values()) {
        if (resource.instance.retired) continue;
        this.scope.run(() => {
          if (resource.record.seeded) resource.controller.activate();
          else resource.controller.release();
        });
      }
    } finally {
      this.release();
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.active = false;
    window.clearTimeout(this.timer);
    this.timer = undefined;
    try {
      this.scope.dispose();
    } finally {
      this.unregister();
      this.release();
      this.wake();
    }
  }

  onScopeWork(): void {
    this.wake();
  }

  onNodeCreated(node: ReactiveNode, _scope: ExecutionScope | undefined, owner: ReactiveNode | undefined): void {
    this.instances.inherit(node, owner);
  }

  onNodeDisposed(node: ReactiveNode): void {
    const resource = this.resources.get(node);
    if (resource !== undefined) this.instances.retire(resource.instance);
    this.wake();
  }

  onResourceRegistered(runtime: RuntimeResource): void {
    this.guard(() => {
      const instance = this.instances.reserve("r", runtime.site, this.instances.current(runtime.owner, runtime.moduleId));
      const input = this.resourceInputs.get(instance.id);
      if (input === undefined || input.ownerId !== instance.id || input.seeded !== (runtime.kind === "public")) {
        throw this.mismatch(`resource seed mismatch ${instance.id}`, runtime.site);
      }
      const controller = controlResource(runtime.node)!;
      const consumer: ResourceConsumer = { instance, record: input, runtime, controller, ready: [], index: 0 };
      this.instances.bind(runtime.node, instance);
      this.resources.set(runtime.node, consumer);
      this.resourcesById.set(instance.id, consumer);
      this.wake();
    });
  }

  shouldSkipInitialProducer(resource: RuntimeResource): boolean {
    return resource.kind === "public";
  }

  shouldHoldSettlement(): boolean {
    return true;
  }

  onResourceState(runtime: RuntimeResource, snapshot: ResourceSnapshot): void {
    this.guard(() => {
      const consumer = this.resources.get(runtime.node);
      if (consumer === undefined) throw this.mismatch("unregistered resource", runtime.site);
      if (snapshot.pending) {
        this.applyResource(consumer, "inline");
      } else {
        consumer.ready.push(snapshot);
        this.wake();
      }
    });
  }

  onFlushBoundary(_scope: ExecutionScope, delivery: FlushDelivery, phase: "before" | "after"): void {
    this.guard(() => {
      if (this.finalizing) return;
      this.consume(phase === "before" ? "flush" : "checkpoint", `flush.${phase}`, this.instances.current().id, delivery);
      if (phase === "before") this.applyPendingResources();
    });
  }

  onModuleAwait(frame: ModuleScopeFrame, site: SourceSite): void {
    this.guard(() => {
      const instance = this.instances.reserve("n", site, this.instances.current(frame.moduleOwner, frame.moduleId));
      this.consume("checkpoint", `${instance.id}.suspend`, instance.id, "inline");
      const gate = Promise.withResolvers<unknown>();
      const consumer: ModuleAwaitConsumer = {
        instance,
        ...gate,
        cancel: this.scope.trackPendingWork(gate.promise),
        ready: false,
        rejected: false,
        released: false,
        value: undefined,
      };
      (this.moduleAwaits ??= new WeakMap()).set(frame, consumer);
      (this.moduleAwaitsById ??= new Map()).set(`${instance.id}.resume`, consumer);
    });
  }

  interceptModuleAwait(frame: ModuleScopeFrame, value: unknown): Promise<unknown> {
    this.check();
    const consumer = this.moduleAwaits?.get(frame);
    if (consumer === undefined) throw this.mismatch("module await was not registered");
    try {
      Promise.resolve(value).then(
        (value) => this.moduleAwaitReady(consumer, value, false),
        (error) => this.moduleAwaitReady(consumer, error, true),
      );
    } catch (error) {
      this.moduleAwaitReady(consumer, error, true);
    }
    return consumer.promise;
  }

  private moduleAwaitReady(consumer: ModuleAwaitConsumer, value: unknown, rejected: boolean): void {
    if (this.failed || this.disposed) return;
    consumer.ready = true;
    consumer.rejected = rejected;
    consumer.value = value;
    this.wake();
  }

  onModuleResume(frame: ModuleScopeFrame): void {
    this.guard(() => {
      const consumer = this.moduleAwaits?.get(frame);
      if (consumer === undefined || !consumer.released) throw this.mismatch("module await resumed before its recorded handoff");
      const id = `${consumer.instance.id}.resume`;
      this.consume("checkpoint", id, consumer.instance.id, "scheduled");
      consumer.cancel();
      this.moduleAwaits?.delete(frame);
      this.moduleAwaitsById?.delete(id);
    });
  }

  onContinuationEvent(handle: ContinuationHandle, event: ContinuationEvent, site: SourceSite | undefined): void {
    this.guard(() => {
      if (event === "begin") {
        const instance = this.instances.reserve("c", site, this.instances.current(handle.owner, handle.moduleId));
        const consumer: ContinuationConsumer = { instance, previous: this.instances.enter(instance), entered: true, pending: undefined };
        this.continuations.set(handle, consumer);
        handle.replaying = true;
        onCleanup(() => {
          consumer.pending?.cancel();
          this.instances.retire(instance);
          this.wake();
        });
        this.consume("checkpoint", `${instance.id}.begin`, instance.id, "inline");
        return;
      }
      const consumer = this.continuations.get(handle);
      if (consumer === undefined) throw this.mismatch("unregistered continuation", site);
      if (event === "resume" || event === "reject") {
        consumer.previous = this.instances.enter(consumer.instance);
        consumer.entered = true;
        consumer.pending = undefined;
        return;
      }
      if (event === "suspend") {
        if (site === undefined) throw this.mismatch("managed await is missing its compiler site");
        const id = `${consumer.instance.id}.a${site.key}`;
        const occurrence = consumer.instance.next(`a${site.key}`);
        this.consume("checkpoint", `${id}_${occurrence.toString(36)}.suspend`, consumer.instance.id, "inline");
        const gate = Promise.withResolvers<unknown>();
        const pending: AwaitConsumer = {
          id,
          occurrence,
          instance: consumer.instance,
          ...gate,
          cancel: this.scope.trackPendingWork(gate.promise),
          released: false,
        };
        consumer.pending = pending;
        this.awaits.set(awaitKey(id, occurrence), pending);
      } else {
        consumer.pending?.cancel();
        consumer.pending = undefined;
        this.consume("checkpoint", `${consumer.instance.id}.end`, consumer.instance.id, "inline");
      }
      if (consumer.entered) {
        this.instances.restore(consumer.previous);
        consumer.previous = undefined;
        consumer.entered = false;
      }
      this.wake();
    });
  }

  expectsAwait(handle: ContinuationHandle, site: SourceSite): boolean {
    this.check();
    const consumer = this.continuations.get(handle);
    if (consumer === undefined) throw this.mismatch("unregistered continuation", site);
    const expected = this.nextHandoff();
    if (expected?.delivery !== "inline" || expected.ownerId !== consumer.instance.id) return false;
    const id = `${consumer.instance.id}.a${site.key}`;
    return expected.kind === "await"
      ? expected.id === id
      : expected.kind === "checkpoint" && expected.id.startsWith(`${id}_`) && expected.id.endsWith(".suspend");
  }

  replayAwaitOperand(handle: ContinuationHandle, site: SourceSite): undefined {
    this.check();
    const consumer = this.continuations.get(handle);
    if (consumer === undefined) throw this.mismatch("unregistered continuation", site);
    const expected = this.nextHandoff();
    const id = `${consumer.instance.id}.a${site.key}`;
    if (expected?.kind !== "await" || expected.delivery !== "inline" || expected.id !== id) return undefined;
    const occurrence = consumer.instance.next(`a${site.key}`);
    const key = awaitKey(id, occurrence);
    const input = this.awaitInputs.get(key);
    if (input?.status !== "thrown") throw this.mismatch(`await operand status mismatch ${key}`, site);
    this.decoder!.apply(input.frame);
    const error = this.decoder!.read(input.value);
    this.consume("await", id, consumer.instance.id, "inline", occurrence);
    this.awaitInputs.delete(key);
    throw error;
  }

  private nextHandoff(): Handoff | undefined {
    let sequence = this.sequence;
    let expected = this.data!.handoffs[sequence];
    while (expected !== undefined && this.retired.has(expected.ownerId) && !this.instances.has(expected.ownerId)) {
      expected = this.data!.handoffs[++sequence];
    }
    return expected;
  }

  interceptSuspend(handle: ContinuationHandle): { held: true; promise: Promise<unknown> } | undefined {
    const pending = this.continuations.get(handle)?.pending;
    return pending === undefined ? undefined : { held: true, promise: pending.promise };
  }

  interceptContinuationValue(handle: ContinuationHandle, event: "resume" | "reject"): { value: unknown } {
    this.check();
    const pending = this.continuations.get(handle)?.pending;
    if (pending === undefined || !pending.released) throw this.mismatch("await resumed before its recorded handoff", handle.site);
    const key = awaitKey(pending.id, pending.occurrence);
    const input = this.awaitInputs.get(key);
    if (input === undefined || input.status !== (event === "resume" ? "resolved" : "rejected")) {
      throw this.mismatch(`await status mismatch ${key}`, handle.site);
    }
    this.decoder!.apply(input.frame);
    const value = this.decoder!.read(input.value);
    this.consume("await", pending.id, pending.instance.id, "scheduled", pending.occurrence);
    pending.cancel();
    this.awaits.delete(key);
    this.awaitInputs.delete(key);
    return { value };
  }

  resolveUniqueId(request: { owner: ReactiveNode | undefined; site?: SourceSite; moduleId: string | undefined }): string {
    return this.instances.uniqueId(request.owner, request.site, request.moduleId);
  }

  protected check(): void {
    if (this.failed) throw this.error;
    if (this.active && Date.now() >= this.deadline) throw this.mismatch("hydration deadline exceeded");
    if (this.disposed) throw new HydrationError("hydration session is disposed");
  }

  protected fail(error: unknown): void {
    if (this.failed) return;
    this.failed = true;
    this.error = error;
    this.failure.reject(error);
    this.wake();
  }

  private mismatch(message: string, site?: SourceSite): HydrationError {
    const error = new HydrationError(`${message} at handoff ${this.sequence} for ${this.data?.pathname ?? "disposed page"}`, site);
    this.fail(error);
    return error;
  }

  private consume(kind: Handoff["kind"], id: string, ownerId: string, delivery: FlushDelivery, index?: number): void {
    this.check();
    let expected = this.data!.handoffs[this.sequence];
    while (
      expected !== undefined &&
      (expected.kind !== kind || expected.id !== id || expected.index !== index) &&
      this.retired.has(expected.ownerId) &&
      !this.instances.has(expected.ownerId)
    ) {
      this.sequence += 1;
      expected = this.data!.handoffs[this.sequence];
    }
    if (
      expected === undefined ||
      expected.kind !== kind ||
      expected.id !== id ||
      expected.ownerId !== ownerId ||
      expected.delivery !== delivery ||
      expected.index !== index
    ) {
      throw this.mismatch(
        `expected ${expected === undefined ? "end of inputs" : `${expected.kind} ${expected.id}`}, received ${kind} ${id}`,
      );
    }
    this.sequence += 1;
    this.wake();
  }

  private applyResource(consumer: ResourceConsumer, delivery: FlushDelivery): void {
    const input = consumer.record.states[consumer.index];
    if (input === undefined) throw this.mismatch(`resource ${consumer.record.id} has extra transitions`, consumer.runtime.site);
    const actual = input.pending || consumer.record.seeded ? undefined : consumer.ready.shift();
    if (!input.pending && !consumer.record.seeded && actual === undefined)
      throw this.mismatch(`resource ${consumer.record.id} is not ready`, consumer.runtime.site);
    if (actual !== undefined && (actual.hasResolved !== input.hasResolved || actual.hasRejection !== input.hasRejection)) {
      throw this.mismatch(`resource ${consumer.record.id} settled differently`, consumer.runtime.site);
    }
    this.decoder!.apply(input.frame);
    const snapshot = this.resourceState(input, consumer, actual);
    this.consume("resource", consumer.record.id, consumer.instance.id, delivery, consumer.index);
    consumer.index += 1;
    consumer.controller.applyState(snapshot);
  }

  private resourceState(input: ResourceStateRecord, consumer: ResourceConsumer, actual: ResourceSnapshot | undefined): ResourceSnapshot {
    return {
      pending: input.pending,
      hasResolved: input.hasResolved,
      hasRejection: input.hasRejection,
      resolved:
        consumer.record.seeded && input.hasResolved
          ? this.decoder!.read(input.resolved!)
          : actual === undefined
            ? consumer.runtime.committed.resolved
            : actual.resolved,
      rejection: input.hasRejection ? this.decoder!.read(input.rejection!) : undefined,
    };
  }

  private applyPendingResources(): void {
    for (;;) {
      const expected = this.data!.handoffs[this.sequence];
      if (expected?.kind !== "resource" || expected.delivery !== "inline") return;
      const consumer = this.resourcesById.get(expected.id);
      if (consumer === undefined || !consumer.record.seeded || consumer.index === 0 || !consumer.record.states[consumer.index]?.pending)
        return;
      this.applyResource(consumer, "inline");
    }
  }

  private advance(): boolean {
    const expected = this.data!.handoffs[this.sequence];
    if (expected === undefined) return false;
    if (this.viewStarted && this.retired.has(expected.ownerId) && !this.instances.has(expected.ownerId)) {
      this.sequence += 1;
      return true;
    }
    if (expected.delivery !== "scheduled") return false;
    if (expected.kind === "flush") {
      if (!this.scope.flush("scheduled", this.deadline, true)) throw this.mismatch("recorded reactive flush timed out");
      return true;
    }
    if (expected.kind === "checkpoint") {
      const consumer = this.moduleAwaitsById?.get(expected.id);
      if (consumer === undefined || !consumer.ready || consumer.released) return false;
      consumer.released = true;
      if (consumer.rejected) consumer.reject(consumer.value);
      else consumer.resolve(consumer.value);
      return false;
    }
    if (expected.kind === "resource") {
      const consumer = this.resourcesById.get(expected.id);
      if (consumer === undefined || (!consumer.record.seeded && consumer.ready.length === 0)) return false;
      this.applyResource(consumer, "scheduled");
      return true;
    }
    if (expected.kind === "await") {
      const key = awaitKey(expected.id, expected.index!);
      const pending = this.awaits.get(key);
      if (pending === undefined || pending.released) return false;
      const input = this.awaitInputs.get(key)!;
      pending.released = true;
      if (input.status === "resolved") pending.resolve(undefined);
      else pending.reject(undefined);
      return true;
    }
    return false;
  }

  private async drive(done: () => boolean, settle: boolean): Promise<void> {
    if (this.driving) throw this.mismatch("overlapping hydration replay drivers");
    this.driving = true;
    try {
      for (;;) {
        const progress = this.progress;
        this.check();
        if (this.advance()) continue;
        if (done()) {
          if (!settle) return;
          this.finalizing = true;
          try {
            if (!this.scope.flush("scheduled", this.deadline)) throw this.mismatch("final reactive flush timed out");
          } finally {
            this.finalizing = false;
          }
          this.check();
          if (this.scope.pendingWork().length === 0 && !this.scope.hasQueuedWork()) return;
        }
        if (progress !== this.progress) continue;
        const changed = (this.changed ??= Promise.withResolvers<void>()).promise;
        await Promise.race([changed, this.failure.promise]);
      }
    } catch (error) {
      this.fail(error);
      throw error;
    } finally {
      this.driving = false;
    }
  }

  private guard(fn: () => void): void {
    if (this.failed || this.disposed || !this.active) return;
    try {
      fn();
    } catch (error) {
      this.fail(error);
    }
  }

  private wake(): void {
    this.progress += 1;
    const changed = this.changed;
    this.changed = undefined;
    changed?.resolve();
  }

  private armDeadline(): void {
    const remaining = this.deadline - Date.now();
    if (remaining <= 0) {
      let error: unknown = new HydrationError(`hydration timed out for ${this.data?.pathname ?? "page"} at handoff ${this.sequence}`);
      try {
        this.dispose();
      } catch (cleanupError) {
        error = new AggregateError([error, cleanupError], "hydration timeout and cleanup failure");
      }
      this.fail(error);
      return;
    }
    this.timer = window.setTimeout(() => this.armDeadline(), Math.min(remaining, 2_147_483_647));
  }

  private release(): void {
    this.data = undefined;
    this.decoder = undefined;
    this.resourceInputs.clear();
    this.routeInputs.clear();
    this.awaitInputs.clear();
    this.resources.clear();
    this.resourcesById.clear();
    this.awaits.clear();
    if (this.moduleAwaitsById !== undefined && this.moduleAwaitsById.size !== 0) {
      const error = this.error ?? new HydrationError("hydration ended during module initialization");
      for (const consumer of this.moduleAwaitsById.values()) {
        consumer.cancel();
        consumer.reject(error);
      }
    }
    this.moduleAwaits = undefined;
    this.moduleAwaitsById = undefined;
    this.retired.clear();
    this.instances.release();
  }
}
