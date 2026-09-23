// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { effectDepth, track } from "./context";
import { FlagDirty, FlagMutable } from "./flags";
import { propagate, type Link, type ReactiveNode, shallowPropagate } from "./graph";
import { batchDepth, scheduleFlush } from "./scheduler";

export type Getter<T> = () => T;
/** Writes `next` (or the result of calling it with the current value) and returns the new value. */
export type Setter<T> = (next: T | ((prev: T) => T)) => T;
export type Equals<T> = false | ((prev: T, next: T) => boolean);
export interface SignalOptions<T> {
  /** Suppresses notification when it returns `true`; `false` always notifies. Default `Object.is`. */
  equals?: Equals<T>;
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
    return this.equals === false || !this.equals(prev, next);
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
  if (this.equals === false || !this.equals(prev, next)) {
    this.pendingValue = next;
    this.flags = FlagMutable | FlagDirty;
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
