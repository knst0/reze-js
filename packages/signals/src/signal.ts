// Ported from alien-signals (MIT, Copyright (c) 2024-present Johnson Chu); see graph.ts.
import { effectDepth, track } from "./context";
import { FlagDirty, FlagMutable } from "./flags";
import { propagate, type Link, type ReactiveNode, shallowPropagate } from "./graph";
import { profileCreated, profileWrote } from "./profile";
import { scheduleFlush } from "./scheduler";

export type Getter<T> = () => T;
/** Writes `next`, or the result of calling it with the latest written value, and returns it. */
export type Setter<T> = (next: T | ((prev: T) => T)) => T;
/** `true` suppresses notification; `false` notifies on every write. */
export type Equals<T> = false | ((prev: T, next: T) => boolean);
export interface SignalOptions<T> {
  /** Default `Object.is`. */
  equals?: Equals<T>;
  /** The name profiling attribution shows; ignored in production builds. */
  name?: string;
}

export class SignalNode<T = unknown> implements ReactiveNode {
  declare subs: Link | undefined;
  declare subsTail: Link | undefined;
  declare flags: number;
  declare currentValue: T;
  declare pendingValue: T;
  declare equals: Equals<T>;

  constructor(value: T, equals: Equals<T>) {
    this.subs = undefined;
    this.subsTail = undefined;
    this.flags = FlagMutable;
    this.currentValue = value;
    this.pendingValue = value;
    this.equals = equals;
  }

  update(): boolean {
    this.flags = FlagMutable;
    const prev = this.currentValue;
    return differs(this.equals, prev, (this.currentValue = this.pendingValue));
  }

  read(): T {
    if (this.flags & FlagDirty && this.update()) {
      const subs = this.subs;
      if (subs !== undefined) {
        shallowPropagate(subs);
      }
    }
    track(this);
    return this.currentValue;
  }

  /** Stores `next` and notifies subscribers unless `equals` reports it equal to the latest write. */
  write(next: T): void {
    if (differs(this.equals, this.pendingValue, next)) {
      const subs = this.subs;
      this.pendingValue = next;
      if (process.env.NODE_ENV !== "production") {
        profileWrote(this);
      }
      if (subs === undefined) {
        this.currentValue = next;
        this.flags = FlagMutable;
        return;
      }
      this.flags = FlagMutable | FlagDirty;
      propagate(subs, effectDepth !== 0);
      scheduleFlush();
    }
  }
}

/**
 * Creates a writable signal. Read and write rights are separate values so the compiler can prove
 * a signal constant by tracking the setter alone. To store a function, pass an updater returning it.
 */
export function signal<T>(): [Getter<T | undefined>, Setter<T | undefined>];
export function signal<T>(initialValue: T, options?: SignalOptions<T>): [Getter<T>, Setter<T>];
export function signal<T>(initialValue?: T, options?: SignalOptions<T | undefined>): [Getter<T | undefined>, Setter<T | undefined>] {
  const node = new SignalNode(initialValue, options?.equals ?? Object.is);
  if (process.env.NODE_ENV !== "production") {
    profileCreated(node, "signal", options?.name);
  }
  return [
    (): T | undefined => node.read(),
    (next: (T | undefined) | ((prev: T | undefined) => T | undefined)): T | undefined => {
      const value = typeof next === "function" ? (next as (prev: T | undefined) => T | undefined)(node.pendingValue) : next;
      node.write(value);
      return value;
    },
  ];
}

/** `!equals(prev, next)` with the default `Object.is` intrinsic inlined. */
export function differs<T>(equals: Equals<T>, prev: T, next: T): boolean {
  if (equals === false) {
    return true;
  }
  if (equals !== Object.is) {
    return !equals(prev, next);
  }
  return !(prev === next
    ? (prev as number) !== 0 || 1 / (prev as number) === 1 / (next as number)
    : (prev as unknown) !== (prev as unknown) && (next as unknown) !== (next as unknown));
}
