import { endTracking, enterEffect, enterOwner, exitEffect, markPure, reportError, setActiveSub, startTracking } from "./context";
import { FlagDirty, FlagOwnsChildren, FlagPending, FlagRecursedCheck, FlagWatching } from "./flags";
import { checkDirty, disposeChildren, disposeNode, type Link, type ReactiveNode } from "./graph";
import { profileCreated, profileReran } from "./profile";

/**
 * DOM binding: a single-phase effect that threads its previous return value into the next run.
 * No disposer handle and no cleanup slot; it lives exactly as long as its owner.
 */
class RenderNode<T> implements ReactiveNode {
  declare deps: Link | undefined;
  declare depsTail: Link | undefined;
  declare subs: Link | undefined;
  declare subsTail: Link | undefined;
  declare flags: number;
  declare fn: (prev: T) => T;
  declare value: T;

  constructor(fn: (prev: T) => T, value: T) {
    this.deps = undefined;
    this.depsTail = undefined;
    this.subs = undefined;
    this.subsTail = undefined;
    this.flags = FlagWatching | FlagRecursedCheck;
    this.fn = fn;
    this.value = value;
  }

  run(): void {
    try {
      this.runIfDirty();
    } catch (error) {
      reportError(this, error);
    }
  }

  runIfDirty(): void {
    const flags = this.flags;
    if (flags & FlagDirty || (flags & FlagPending && checkDirty(this.deps!, this))) {
      if (flags & FlagOwnsChildren) {
        disposeChildren(this);
      }
      if (process.env.NODE_ENV !== "production") {
        profileReran(this);
      }
      const prevSub = startTracking(this, FlagWatching);
      try {
        enterEffect();
        this.value = this.fn(this.value);
      } finally {
        exitEffect();
        endTracking(this, prevSub);
      }
    } else if (this.deps !== undefined) {
      this.flags = FlagWatching | (flags & FlagOwnsChildren);
    }
  }

  unwatched(): void {
    disposeNode(this);
  }

  dispose(): void {
    disposeNode(this);
  }
}

/**
 * Runs `fn(init)` now and again whenever what it read changes, passing the previous result.
 * A binding that read nothing reactive and created nothing is dropped right away, so static
 * expressions cost no graph node.
 */
export function renderEffect<T>(fn: (prev: T) => T, init?: T): void {
  const node = new RenderNode(fn, init as T);
  if (process.env.NODE_ENV !== "production") {
    markPure(node);
    profileCreated(node, "render", undefined);
  }
  const prevSub = enterOwner(node);
  try {
    enterEffect();
    node.value = fn(node.value);
  } finally {
    exitEffect();
    setActiveSub(prevSub);
    node.flags &= ~FlagRecursedCheck;
  }
  if (node.deps === undefined) {
    disposeNode(node);
  }
}

/**
 * Render binding whose reactive reads are the same on every run. The first run tracks; reruns
 * in production skip tracking. Development reruns re-track and warn when the reads change.
 */
class FixedRenderNode implements ReactiveNode {
  declare deps: Link | undefined;
  declare depsTail: Link | undefined;
  declare subs: Link | undefined;
  declare subsTail: Link | undefined;
  declare flags: number;
  declare fn: () => void;

  constructor(fn: () => void) {
    this.deps = undefined;
    this.depsTail = undefined;
    this.subs = undefined;
    this.subsTail = undefined;
    this.flags = FlagWatching | FlagRecursedCheck;
    this.fn = fn;
  }

  run(): void {
    try {
      this.runIfDirty();
    } catch (error) {
      reportError(this, error);
    }
  }

  runIfDirty(): void {
    const flags = this.flags;
    if (flags & FlagDirty || (flags & FlagPending && checkDirty(this.deps!, this))) {
      if (process.env.NODE_ENV !== "production") {
        profileReran(this);
        rerunChecked(this);
        return;
      }
      this.flags = FlagWatching | FlagRecursedCheck | (flags & FlagOwnsChildren);
      const prevSub = setActiveSub(undefined);
      try {
        enterEffect();
        this.fn();
      } finally {
        exitEffect();
        setActiveSub(prevSub);
        this.flags &= ~FlagRecursedCheck;
      }
    } else {
      this.flags = FlagWatching | (flags & FlagOwnsChildren);
    }
  }

  unwatched(): void {
    disposeNode(this);
  }

  dispose(): void {
    disposeNode(this);
  }
}

let warnedFixed: WeakSet<ReactiveNode> | undefined;

function rerunChecked(node: FixedRenderNode): void {
  if (node.flags & FlagOwnsChildren) {
    disposeChildren(node);
  }
  const before: ReactiveNode[] = [];
  for (let link = node.deps; link !== undefined; link = link.nextDep) {
    before.push(link.dep);
  }
  const prevSub = startTracking(node, FlagWatching);
  try {
    enterEffect();
    node.fn();
  } finally {
    exitEffect();
    endTracking(node, prevSub);
  }
  let link = node.deps;
  let i = 0;
  while (link !== undefined && i < before.length && link.dep === before[i]) {
    link = link.nextDep;
    i++;
  }
  if ((link !== undefined || i !== before.length) && !(warnedFixed ??= new WeakSet()).has(node)) {
    warnedFixed.add(node);
    console.warn(
      "[rezejs] A compiled binding read a different set of reactive values, or created a reactive node, than on its first run. " +
        "Production builds do not re-track compiled bindings, so it would miss updates. " +
        "Do not read signals inside toString, valueOf or getters of values bound to attributes.\n" +
        `Binding: ${node.fn.toString().slice(0, 300)}`,
    );
  }
}

/**
 * {@link renderEffect} for a binding whose reactive reads are the same on every run; the compiler
 * emits it. Development builds re-track and warn when the reads change.
 */
export function fixedRenderEffect(fn: () => void): void {
  const node = new FixedRenderNode(fn);
  if (process.env.NODE_ENV !== "production") {
    markPure(node);
    profileCreated(node, "render", undefined);
  }
  const prevSub = enterOwner(node);
  try {
    enterEffect();
    fn();
  } finally {
    exitEffect();
    setActiveSub(prevSub);
    node.flags &= ~FlagRecursedCheck;
  }
  if (node.deps === undefined) {
    disposeNode(node);
  }
}
