import { getOwner, setActiveOwner, setActiveSub } from "../context";
import type { ReactiveNode } from "../graph";
import { takeSeed, type Seed } from "../island";
import {
  enterScopeContext,
  getActiveScope,
  getScopeObserver,
  restoreScopeContext,
  scopeOfNode,
  type ExecutionScope,
  type ScopeContext,
  type SourceSite,
} from "./scope";

export type ContinuationEvent = "begin" | "suspend" | "resume" | "reject" | "end";

export interface ContinuationHandle {
  readonly site: SourceSite | undefined;
  readonly scope: ExecutionScope | undefined;
  readonly owner: ReactiveNode | undefined;
  suspend<T>(value: T, awaitSite?: SourceSite): T | Promise<T>;
  suspendLazy<T>(operand: () => T, awaitSite?: SourceSite): T | Promise<unknown>;
  resume<T>(value: T): T;
  reject<T>(error: T): T;
  end(): void;
}

enum State {
  Idle = 0,
  Suspended = 1,
  Resumed = 2,
  Ended = 3,
}

class Continuation implements ContinuationHandle {
  declare readonly site: SourceSite | undefined;
  declare readonly scope: ExecutionScope | undefined;
  readonly owner: ReactiveNode | undefined;
  private previousOwner: ReactiveNode | undefined;
  private previousSubscriber: ReactiveNode | undefined;
  declare private previousScope: ScopeContext | undefined;
  private state: State = State.Idle;
  declare private replay: Seed | undefined;
  private position = 0;

  constructor(site: SourceSite | string | undefined) {
    const owner = getOwner();
    this.owner = owner;
    if (__REZE_HTML__) {
      this.site = site as SourceSite | undefined;
      this.scope = getActiveScope() ?? (owner !== undefined ? scopeOfNode(owner) : undefined);
      getScopeObserver()?.onContinuationEvent?.(this, "begin", this.site, undefined);
    } else if (typeof site === "string") {
      this.replay = takeSeed(site);
    }
  }

  suspendLazy<T>(operand: () => T, awaitSite?: SourceSite): T | Promise<unknown> {
    const replay = this.replay;
    let recorded: Promise<unknown> | undefined;
    if (replay !== undefined) {
      if (this.position < replay.values.length) recorded = Promise.resolve(replay.values[this.position++]);
      else if (replay.rejection !== undefined) recorded = Promise.reject(replay.rejection.error);
    }
    return this.suspend(recorded ?? operand(), awaitSite);
  }

  suspend<T>(value: T, awaitSite?: SourceSite): T | Promise<T> {
    if (this.state === State.Resumed) {
      setActiveSub(this.previousSubscriber);
      setActiveOwner(this.previousOwner);
      this.previousSubscriber = undefined;
      this.previousOwner = undefined;
      if (__REZE_HTML__ && this.previousScope !== undefined) {
        restoreScopeContext(this.previousScope);
        this.previousScope = undefined;
      }
      this.state = State.Suspended;
    } else if (this.state === State.Idle) {
      this.state = State.Suspended;
    }
    if (__REZE_HTML__ && this.state === State.Suspended) {
      getScopeObserver()?.onContinuationEvent?.(this, "suspend", awaitSite ?? this.site, value);
    }
    return value;
  }

  resume<T>(value: T): T {
    if (this.state === State.Suspended || this.state === State.Idle) {
      this.previousSubscriber = setActiveSub(undefined);
      this.previousOwner = setActiveOwner(this.owner);
      this.state = State.Resumed;
      if (__REZE_HTML__) {
        this.previousScope = enterScopeContext(this.scope);
        getScopeObserver()?.onContinuationEvent?.(this, "resume", this.site, value);
      }
    }
    return value;
  }

  reject<T>(error: T): T {
    if (this.state === State.Suspended) {
      this.previousSubscriber = setActiveSub(undefined);
      this.previousOwner = setActiveOwner(this.owner);
      this.state = State.Resumed;
      if (__REZE_HTML__) {
        this.previousScope = enterScopeContext(this.scope);
        getScopeObserver()?.onContinuationEvent?.(this, "reject", this.site, error);
      }
    }
    return error;
  }

  end(): void {
    if (this.state === State.Ended) {
      return;
    }
    if (this.state === State.Resumed) {
      setActiveSub(this.previousSubscriber);
      setActiveOwner(this.previousOwner);
      this.previousSubscriber = undefined;
      this.previousOwner = undefined;
      if (__REZE_HTML__ && this.previousScope !== undefined) {
        restoreScopeContext(this.previousScope);
        this.previousScope = undefined;
      }
    }
    this.state = State.Ended;
    if (__REZE_HTML__) {
      getScopeObserver()?.onContinuationEvent?.(this, "end", this.site, undefined);
    }
  }
}

export function beginContinuation(site?: SourceSite | string): ContinuationHandle {
  return new Continuation(site);
}
