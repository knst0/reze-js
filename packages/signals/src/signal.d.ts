import { type Link, type ReactiveNode } from "./graph";
export type Getter<T> = () => T;
/** Writes `next`, or the result of calling it with the latest written value, and returns it. */
export type Setter<T> = (next: T | ((prev: T) => T)) => T;
/** `true` suppresses notification; `false` notifies on every write. */
export type Equals<T> = false | ((prev: T, next: T) => boolean);
export interface SignalOptions<T> {
    /** Default `Object.is`. */
    equals?: Equals<T>;
    /** The name devtools show; ignored in production builds. */
    name?: string;
}
export declare class SignalNode<T = unknown> implements ReactiveNode {
    subs: Link | undefined;
    subsTail: Link | undefined;
    flags: number;
    currentValue: T;
    pendingValue: T;
    equals: Equals<T>;
    constructor(value: T, equals: Equals<T>);
    update(): boolean;
    read(): T;
    /** Stores `next` and notifies subscribers unless `equals` reports it equal to the latest write. */
    write(next: T): void;
}
/**
 * Creates a writable signal. Read and write rights are separate values so the compiler can prove
 * a signal constant by tracking the setter alone. To store a function, pass an updater returning it.
 */
export declare function signal<T>(): [Getter<T | undefined>, Setter<T | undefined>];
export declare function signal<T>(initialValue: T, options?: SignalOptions<T>): [Getter<T>, Setter<T>];
/** `!equals(prev, next)` with the default `Object.is` intrinsic inlined. */
export declare function differs<T>(equals: Equals<T>, prev: T, next: T): boolean;
