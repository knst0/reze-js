import type { SignalOptions } from "./signal";

/**
 * Compiler syntax for a reactive variable: `let count = $signal(0)`. The compiler turns reads
 * into getter calls and writes into setter calls. Declare it with `let` or `const` in the file
 * that uses it; it cannot be exported, destructured or passed around.
 */
export function $signal<T>(): T | undefined;
export function $signal<T>(initialValue: T, options?: SignalOptions<T>): T;
export function $signal(): never {
  throw new Error("`$signal()` requires the reze compiler");
}
