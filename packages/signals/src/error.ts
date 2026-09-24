import {
  adopt,
  getOwner,
  parentOwner,
  setActiveOwner,
  setActiveSub,
  setErrorHook,
} from "./context";
import { FlagNone } from "./flags";
import { disposeNode, type Link, type ReactiveNode } from "./graph";

class ErrorHandlerNode implements ReactiveNode {
  deps: Link | undefined = undefined;
  depsTail: Link | undefined = undefined;
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  flags: number = FlagNone;
  handler: (error: unknown) => void;

  constructor(handler: (error: unknown) => void) {
    this.handler = handler;
  }

  dispose(): void {
    disposeNode(this);
  }

  unwatched(): void {
    this.dispose();
  }
}

function handleError(node: ReactiveNode, error: unknown): boolean {
  for (let owner = parentOwner(node); owner !== undefined; owner = parentOwner(owner)) {
    if (owner instanceof ErrorHandlerNode) {
      owner.handler(error);
      return true;
    }
  }
  return false;
}

/**
 * Runs `fn` untracked under an owner that catches errors: a throw from `fn` itself returns
 * `undefined`, and a throw from any effect, binding or computed created inside goes to `handler`
 * too (a computed then keeps its previous value). Nested `catchError`s catch first; errors a handler throws go outward.
 * Lives as long as the current owner.
 */
export function catchError<T>(fn: () => T, handler: (error: unknown) => void): T | undefined {
  setErrorHook(handleError);
  const node = new ErrorHandlerNode(handler);
  const owner = getOwner();
  if (owner !== undefined) {
    adopt(node, owner);
  }
  const prevSub = setActiveSub(undefined);
  const prevOwner = setActiveOwner(node);
  try {
    return fn();
  } catch (error) {
    handler(error);
    return undefined;
  } finally {
    setActiveSub(prevSub);
    setActiveOwner(prevOwner);
  }
}
