import { adopt, getOwner, parentOwner, runWithOwner } from "./context";
import { FlagNone } from "./flags";
import { disposeNode, type Link, type ReactiveNode } from "./graph";
import { SignalNode } from "./signal";

export class ReloadScope implements ReactiveNode {
  declare deps: Link | undefined;
  declare depsTail: Link | undefined;
  declare subs: Link | undefined;
  declare subsTail: Link | undefined;
  declare flags: number;
  declare reloads: SignalNode<number>;

  constructor() {
    this.deps = undefined;
    this.depsTail = undefined;
    this.subs = undefined;
    this.subsTail = undefined;
    this.flags = FlagNone;
    this.reloads = new SignalNode<number>(0, Object.is);
  }

  isPending(): boolean {
    return this.reloads.read() > 0;
  }

  dispose(): void {
    disposeNode(this);
  }

  unwatched(): void {
    this.dispose();
  }
}

/**
 * Runs `fn` under a new owner that lives as long as the current one. `fn` receives a tracked
 * `isPending` that is true while an async computation under the scope, at any depth, re-runs
 * after its first run settled.
 */
export function withReloadScope<T>(fn: (isPending: () => boolean) => T): T {
  const scope = new ReloadScope();
  const owner = getOwner();
  if (owner !== undefined) {
    adopt(scope, owner);
  }
  return runWithOwner(scope, () => fn(() => scope.isPending()));
}

/** Counts a reload of `node` in every enclosing scope; returns them so the reload can be released. */
export function enterReload(node: ReactiveNode): ReloadScope[] | undefined {
  let scopes: ReloadScope[] | undefined;
  for (let owner: ReactiveNode | undefined = node; owner !== undefined; owner = parentOwner(owner)) {
    if (owner instanceof ReloadScope) {
      (scopes ??= []).push(owner);
      owner.reloads.write(owner.reloads.pendingValue + 1);
    }
  }
  return scopes;
}

export function leaveReload(scopes: ReloadScope[]): void {
  for (const scope of scopes) {
    scope.reloads.write(scope.reloads.pendingValue - 1);
  }
}
