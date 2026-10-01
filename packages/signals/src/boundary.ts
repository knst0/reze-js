import { adopt, getOwner, parentOwner, runWithOwner, setPendingReadHook } from "./context";
import { FlagNone } from "./flags";
import { disposeNode, type Link, type ReactiveNode } from "./graph";
import { SignalNode } from "./signal";

export interface Boundary {
  /** Tracked: whether a computation created inside has read an `asyncComputed` whose first run has not settled, and has not re-run or been disposed since. */
  isPending(): boolean;
  /** Disposes everything created inside. */
  dispose(): void;
}

class BoundaryNode implements ReactiveNode, Boundary {
  deps: Link | undefined;
  depsTail: Link | undefined;
  subs: Link | undefined;
  subsTail: Link | undefined;
  flags: number;
  pendingReads: SignalNode<number>;

  constructor() {
    this.deps = undefined;
    this.depsTail = undefined;
    this.subs = undefined;
    this.subsTail = undefined;
    this.flags = FlagNone;
    this.pendingReads = new SignalNode<number>(0, Object.is);
  }

  isPending(): boolean {
    return this.pendingReads.read() > 0;
  }

  dispose(): void {
    disposeNode(this);
  }

  unwatched(): void {
    this.dispose();
  }
}

class PendingReadNode implements ReactiveNode {
  subs: Link | undefined;
  subsTail: Link | undefined;
  flags: number;
  boundary: BoundaryNode;

  constructor(boundary: BoundaryNode) {
    this.subs = undefined;
    this.subsTail = undefined;
    this.flags = FlagNone;
    this.boundary = boundary;
  }

  dispose(): void {}

  unwatched(): void {
    const count = this.boundary.pendingReads;
    count.write(count.pendingValue - 1);
  }
}

function registerPendingRead(reader: ReactiveNode): void {
  for (let owner: ReactiveNode | undefined = reader; owner !== undefined; owner = parentOwner(owner)) {
    if (owner instanceof BoundaryNode) {
      adopt(new PendingReadNode(owner), reader);
      owner.pendingReads.write(owner.pendingReads.pendingValue + 1);
      return;
    }
  }
}

/**
 * Runs `fn` untracked under a new owner that lives as long as the current one and counts pending
 * first loads read inside it; the nearest boundary counts a read.
 */
export function boundary<T>(fn: () => T): [T, Boundary] {
  setPendingReadHook(registerPendingRead);
  const node = new BoundaryNode();
  const owner = getOwner();
  if (owner !== undefined) {
    adopt(node, owner);
  }
  return [runWithOwner(node, fn), node];
}

/** Whether the current owner is inside a `boundary`. */
export function isInBoundary(): boolean {
  for (let owner = getOwner(); owner !== undefined; owner = parentOwner(owner)) {
    if (owner instanceof BoundaryNode) {
      return true;
    }
  }
  return false;
}
