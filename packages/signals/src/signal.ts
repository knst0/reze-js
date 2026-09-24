// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { effectDepth, track } from "./context";
import { debugHook } from "./devtools";
import { FlagDirty, FlagMutable } from "./flags";
import { propagate, type Link, type ReactiveNode, shallowPropagate } from "./graph";
import { batchDepth, scheduleFlush } from "./scheduler";

export type Getter<T> = () => T;
/** Writes `next` (or the result of calling it with the current value) and returns the new value. */
export type Setter<T> = (next: T | ((prev: T) => T)) => T;
/** A signal's read end on its own: the `Getter` half of `signal()` (D08). */
export type ReadonlySignal<T> = Getter<T>;
export type Equals<T> = false | ((prev: T, next: T) => boolean);
export interface SignalOptions<T> {
  /** Suppresses notification when it returns `true`; `false` always notifies. Default `Object.is`. */
  equals?: Equals<T>;
  /** The name devtools show; ignored in production builds. */
  name?: string;
}

class SignalNode<T = unknown> implements ReactiveNode {
  subs: Link | undefined = undefined;
  subsTail: Link | undefined = undefined;
  flags: number = FlagMutable;
  currentValue: T;
  pendingValue: T;
  equals: Equals<T>;

  constructor(value: T, equals: Equals<T>) {
    this.currentValue = value;
    this.pendingValue = value;
    this.equals = equals;
  }

  update(): boolean {
    this.flags = FlagMutable;
    const prev = this.currentValue;
    const next = (this.currentValue = this.pendingValue);
    return differs(this.equals, prev, next);
  }
}

/**
 * Creates a writable signal. Read and write rights are separate values so the compiler can
 * prove a signal constant by tracking the setter alone (ADR-0004).
 */
export function signal<T>(): [Getter<T | undefined>, Setter<T | undefined>];
export function signal<T>(initialValue: T, options?: SignalOptions<T>): [Getter<T>, Setter<T>];
export function signal<T>(
  initialValue?: T,
  options?: SignalOptions<T | undefined>,
): [Getter<T | undefined>, Setter<T | undefined>] {
  const node = new SignalNode(initialValue, options?.equals ?? Object.is);
  if (process.env.NODE_ENV !== "production" && debugHook !== undefined) {
    debugHook.created(node, "signal", options?.name, () => node.pendingValue);
  }
  return [(signalGet<T | undefined>).bind(node), (signalSet<T | undefined>).bind(node)];
}

/** Whether `fn` is a signal getter. */
export function isSignal(fn: () => unknown): boolean {
  return fn.name === "bound " + signalGet.name;
}

function signalGet<T>(this: SignalNode<T>): T {
  if (this.flags & FlagDirty && this.update()) {
    const subs = this.subs;
    if (subs !== undefined) {
      shallowPropagate(subs);
    }
  }
  track(this);
  return this.currentValue;
}

function signalSet<T>(this: SignalNode<T>, next: T | ((prev: T) => T)): T {
  const prev = this.pendingValue;
  if (typeof next === "function") {
    next = (next as (prev: T) => T)(prev);
  }
  if (differs(this.equals, prev, next)) {
    this.pendingValue = next;
    this.flags = FlagMutable | FlagDirty;
    if (process.env.NODE_ENV !== "production" && debugHook !== undefined) {
      debugHook.written(this);
    }
    const subs = this.subs;
    if (subs !== undefined) {
      propagate(subs, effectDepth !== 0);
      if (!batchDepth) {
        scheduleFlush();
      }
    }
  }
  return next;
}

/** `!equals(prev, next)` with the default `Object.is` intrinsic inlined (P06). */
function differs<T>(equals: Equals<T>, prev: T, next: T): boolean {
  if (equals === false) {
    return true;
  }
  if (equals !== Object.is) {
    return !equals(prev, next);
  }
  // `Object.is`: `+0`/`-0` differ, `NaN` equals itself. The division only runs
  // when both are `±0`.
  return !(prev === next
    ? (prev as number) !== 0 || 1 / (prev as number) === 1 / (next as number)
    : (prev as unknown) !== (prev as unknown) && (next as unknown) !== (next as unknown));
}
