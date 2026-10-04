import { getOwner, setActiveOwner, setActiveSub } from "../context";
import type { ReactiveNode } from "../graph";
import {
  currentModuleId,
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
  readonly moduleId: string | undefined;
  readonly owner: ReactiveNode | undefined;
  replaying?: boolean;
  suspend<T>(value: T, awaitSite?: SourceSite): T | Promise<T>;
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
  declare readonly moduleId: string | undefined;
  readonly owner: ReactiveNode | undefined;
  declare replaying?: boolean;
  private previousOwner: ReactiveNode | undefined;
  private previousSubscriber: ReactiveNode | undefined;
  declare private previousScope: ScopeContext | undefined;
  private state: State = State.Idle;

  constructor(site: SourceSite | undefined) {
    const owner = getOwner();
    this.owner = owner;
    if (__REZE_HTML__ || __REZE_HYDRATE__) {
      this.site = site;
      this.replaying = false;
      this.scope = getActiveScope() ?? (owner !== undefined ? scopeOfNode(owner) : undefined);
      this.moduleId = currentModuleId() ?? site?.module;
      getScopeObserver()?.onContinuationEvent?.(this, "begin", site, undefined);
    }
  }

  suspend<T>(value: T, awaitSite?: SourceSite): T | Promise<T> {
    const selected = __REZE_HTML__ || __REZE_HYDRATE__;
    if (this.state === State.Resumed) {
      setActiveSub(this.previousSubscriber);
      setActiveOwner(this.previousOwner);
      this.previousSubscriber = undefined;
      this.previousOwner = undefined;
      if (selected && this.previousScope !== undefined) {
        restoreScopeContext(this.previousScope);
        this.previousScope = undefined;
      }
      this.state = State.Suspended;
    } else if (this.state === State.Idle) {
      this.state = State.Suspended;
    }
    if (selected && this.state === State.Suspended) {
      const observer = getScopeObserver();
      observer?.onContinuationEvent?.(this, "suspend", awaitSite ?? this.site, value);
      const held = observer?.interceptSuspend?.(this, awaitSite ?? this.site, value);
      if (held !== undefined) {
        return held.promise as Promise<T>;
      }
    }
    return value;
  }

  resume<T>(value: T): T {
    if (this.state === State.Suspended || this.state === State.Idle) {
      this.previousSubscriber = setActiveSub(undefined);
      this.previousOwner = setActiveOwner(this.owner);
      this.state = State.Resumed;
      if (__REZE_HTML__ || __REZE_HYDRATE__) {
        this.previousScope = enterScopeContext(this.scope, this.moduleId);
        const observer = getScopeObserver();
        const intercepted = observer?.interceptContinuationValue?.(this, "resume", value);
        if (intercepted !== undefined) value = intercepted.value as T;
        observer?.onContinuationEvent?.(this, "resume", this.site, value);
      }
    }
    return value;
  }

  reject<T>(error: T): T {
    if (this.state === State.Suspended) {
      this.previousSubscriber = setActiveSub(undefined);
      this.previousOwner = setActiveOwner(this.owner);
      this.state = State.Resumed;
      if (__REZE_HTML__ || __REZE_HYDRATE__) {
        this.previousScope = enterScopeContext(this.scope, this.moduleId);
        const observer = getScopeObserver();
        const intercepted = observer?.interceptContinuationValue?.(this, "reject", error);
        if (intercepted !== undefined) error = intercepted.value as T;
        observer?.onContinuationEvent?.(this, "reject", this.site, error);
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
      if ((__REZE_HTML__ || __REZE_HYDRATE__) && this.previousScope !== undefined) {
        restoreScopeContext(this.previousScope);
        this.previousScope = undefined;
      }
    }
    this.state = State.Ended;
    if (__REZE_HTML__ || __REZE_HYDRATE__) {
      getScopeObserver()?.onContinuationEvent?.(this, "end", this.site, undefined);
    }
  }
}

export function beginContinuation(site?: SourceSite): ContinuationHandle {
  return new Continuation(site);
}

export function setContinuationReplaying(handle: ContinuationHandle, replaying: boolean): void {
  handle.replaying = replaying;
}
