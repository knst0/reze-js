import type { ComputedOptions } from "./computed";

/**
 * Compiler syntax for a derived variable: `const doubled = $computed(count * 2)`. The compiler
 * wraps the expression into `computed(() => …)` and turns reads into getter calls. Pass the
 * expression itself, not a function. Declare it in the file that uses it; it cannot be written,
 * exported, destructured or passed around.
 */
export function $computed<T>(value: T, options?: ComputedOptions): T;
export function $computed(): never {
  throw new Error("`$computed()` requires the reze compiler");
}
