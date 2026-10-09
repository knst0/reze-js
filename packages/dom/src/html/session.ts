import { getOwner, onCleanup, root, runWithOwner, type Boundary } from "@rezejs/signals";
import type { ContinuationEvent, ContinuationHandle } from "@rezejs/signals/internal/continuation";
import { createScope, ScopeTimeoutError, type ExecutionScope, type ReactiveNode, type SourceSite } from "@rezejs/signals/internal/scope";

import { beginSeed, type IslandRoot, type Seed } from "../server/island";
import { hComponent } from "./helpers";
import { Instances, registerExecution, type ExecutionObserver, type Instance } from "./instances";
import { RenderError } from "./site";
import type { DirtySink, HtmlRange } from "./tree";
import { trackSink } from "./tree";

export interface HtmlSessionOptions {
  readonly pathname: string;
  readonly rootId: string;
  readonly buildId: string;
  readonly timeoutMs?: number;
  readonly streaming?: boolean;
}

interface RecordedContinuation {
  readonly instance: Instance;
  readonly seed: Seed | undefined;
  previous: Instance | undefined;
  entered: boolean;
  suspended?: () => void;
}

export class HtmlSession implements ExecutionObserver, DirtySink {
  readonly scope: ExecutionScope;
  readonly instances = new Instances();
  readonly moduleStates = new Map<string, object>();
  readonly portals: { node: HtmlRange; instance: Instance; placement: "body" | "inert" }[] = [];
  readonly islands = new Set<IslandRoot>();
  readonly pendingRanges = new Map<HtmlRange, () => boolean>();
  readonly dirtyRanges = new Set<HtmlRange>();
  readonly streaming: boolean;
  mount: HtmlRange | undefined;
  rootBoundary: Boundary | undefined;
  shellFlushed = false;
  private readonly owner: ReactiveNode;
  private readonly moduleOwner: ReactiveNode;
  private readonly unregister: () => void;
  private readonly deadline: number;
  private readonly timeoutMs: number;
  private readonly continuations = new WeakMap<ContinuationHandle, RecordedContinuation>();
  private readonly failure = Promise.withResolvers<never>();
  private nextToken = 0;
  private nextIsland = 0;
  private failed = false;
  private error: unknown;
  private disposed = false;

  constructor(private readonly options: HtmlSessionOptions) {
    this.timeoutMs = options.timeoutMs ?? 30_000;
    this.streaming = options.streaming ?? false;
    this.scope = createScope({ name: options.pathname, controlled: true, timeoutMs: this.timeoutMs });
    this.deadline = Date.now() + this.timeoutMs;
    this.unregister = registerExecution(this);
    this.owner = this.scope.run(() => root(() => getOwner()!));
    this.moduleOwner = this.scope.run(() => runWithOwner(this.owner, () => root(() => getOwner()!)));
    this.instances.bind(this.owner, this.instances.root);
    this.instances.bind(this.moduleOwner, this.instances.root);
    this.failure.promise.catch(() => {});
  }

  get rootId(): string {
    return this.options.rootId;
  }

  get buildId(): string {
    return this.options.buildId;
  }

  get pathname(): string {
    return this.options.pathname;
  }

  run<T>(fn: () => T): T {
    this.check();
    return this.scope.run(() => runWithOwner(this.owner, fn));
  }

  runModule<T>(fn: () => T): T {
    this.check();
    return this.scope.run(() => runWithOwner(this.moduleOwner, fn));
  }

  markShellFlushed(): void {
    if (this.shellFlushed) return;
    this.shellFlushed = true;
    this.mount!.sink = this;
    trackSink(1);
  }

  add(range: HtmlRange): void {
    this.dirtyRanges.add(range);
  }

  islandPrefix(): string {
    return `i${(this.nextIsland++).toString(36)}`;
  }

  tokenOf(range: HtmlRange): string {
    return (range.wire ??= (this.nextToken++).toString(36));
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

  flush(): void {
    this.check();
    this.scope.flush("scheduled");
    this.check();
  }

  isShellReady(): boolean {
    return !this.scope.hasQueuedWork() && this.rootBoundary?.isPending() !== true;
  }

  hasPendingWork(): boolean {
    return this.scope.hasQueuedWork() || this.scope.pendingWork().length !== 0;
  }

  async waitForWork(): Promise<void> {
    this.check();
    const remaining = this.deadline - Date.now();
    if (remaining <= 0) throw new ScopeTimeoutError(this.options.pathname, this.timeoutMs);
    await Promise.race([this.scope.waitForWork(remaining), this.failure.promise]);
    this.check();
  }

  remainingMs(): number {
    return this.deadline - Date.now();
  }

  async settle(): Promise<void> {
    this.check();
    const remaining = this.deadline - Date.now();
    if (remaining <= 0) throw new ScopeTimeoutError(this.options.pathname, this.timeoutMs);
    await Promise.race([this.scope.settle(remaining), this.failure.promise]);
    this.check();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    if (this.shellFlushed) trackSink(-1);
    try {
      this.scope.dispose();
    } finally {
      this.unregister();
    }
  }

  onNodeCreated(node: ReactiveNode, _scope: ExecutionScope | undefined, owner: ReactiveNode | undefined): void {
    this.instances.inherit(node, owner);
  }

  onContinuationEvent(handle: ContinuationHandle, event: ContinuationEvent, site: SourceSite | undefined, value: unknown): void {
    this.guard(() => {
      if (event === "begin") {
        const instance = this.instances.reserve("c", site, this.instances.current(handle.owner));
        const seed = site === undefined ? undefined : beginSeed(instance, site.key);
        const record: RecordedContinuation = { instance, seed, previous: this.instances.enter(instance), entered: true };
        this.continuations.set(handle, record);
        onCleanup(() => {
          record.suspended?.();
          this.instances.retire(instance);
        });
        return;
      }
      const record = this.continuations.get(handle);
      if (record === undefined) throw new RenderError("continuation was not registered", site);
      if (event === "resume" || event === "reject") {
        if (record.suspended === undefined) throw new RenderError("continuation resumed without an await slot", site);
        record.suspended();
        record.previous = this.instances.enter(record.instance);
        record.entered = true;
        if (record.seed !== undefined) {
          if (event === "resume") record.seed.values.push(value);
          else record.seed.rejection = { error: value };
        }
        return;
      }
      if (event === "suspend") {
        const gate = Promise.withResolvers<void>();
        const cancel = this.scope.trackPendingWork(gate.promise);
        record.suspended = () => {
          record.suspended = undefined;
          cancel();
          gate.resolve();
        };
      } else {
        record.suspended?.();
      }
      if (record.entered) {
        this.instances.restore(record.previous);
        record.previous = undefined;
        record.entered = false;
      }
    });
  }

  renderComponent(component: (props: never) => unknown, props: unknown): unknown {
    return hComponent(component as (props: unknown) => unknown, props);
  }

  resolveUniqueId(request: { owner: ReactiveNode | undefined; site?: SourceSite }): string {
    const island = this.instances.current(request.owner).island;
    if (island !== undefined) return `${island.prefix}-${island.nextId++}`;
    return this.instances.uniqueId(request.owner, request.site);
  }

  private guard(fn: () => void): void {
    if (this.failed || this.disposed) return;
    try {
      fn();
    } catch (error) {
      this.fail(error);
    }
  }

  fail(error: unknown): void {
    if (this.failed) return;
    this.failed = true;
    this.error = error;
    this.failure.reject(error);
  }

  private check(): void {
    if (this.failed) throw this.error;
    if (this.disposed) throw new RenderError("HTML session is disposed");
  }
}
