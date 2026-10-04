import { getOwner, setActiveOwner, setActiveSub } from "../context";
import type { ReactiveNode } from "../graph";

class Continuation {
  private readonly owner: ReactiveNode | undefined;
  private previousOwner: ReactiveNode | undefined;
  private previousSubscriber: ReactiveNode | undefined;
  private isResumed: boolean;

  constructor() {
    this.owner = getOwner();
    this.previousOwner = undefined;
    this.previousSubscriber = undefined;
    this.isResumed = false;
  }

  suspend<T>(value: T): T {
    this.end();
    return value;
  }

  resume<T>(value: T): T {
    if (!this.isResumed) {
      this.previousSubscriber = setActiveSub(undefined);
      this.previousOwner = setActiveOwner(this.owner);
      this.isResumed = true;
    }
    return value;
  }

  reject<T>(error: T): T {
    return this.resume(error);
  }

  end(): void {
    if (this.isResumed) {
      setActiveSub(this.previousSubscriber);
      setActiveOwner(this.previousOwner);
      this.previousSubscriber = undefined;
      this.previousOwner = undefined;
      this.isResumed = false;
    }
  }
}

export function beginContinuation(): Continuation {
  return new Continuation();
}
