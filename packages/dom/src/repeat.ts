import { computed } from "@rezejs/signals";

import type { JSX } from "./jsx";
import { list } from "./list";

function indexes(length: number): number[] {
  return Array.from({ length }, (_, index) => index);
}

/**
 * Runtime of `<Repeat>`: `map(index)` once for each index in `0..count()`, in its own root. Rows keep their nodes while
 * `count()` changes: a smaller count disposes the last rows, a larger one appends. A `count()` that is not a positive
 * number is `0`, a fraction is truncated. `fallback` is shown while there are no rows.
 */
export function repeat(count: () => number, map: (index: number) => JSX.Element, fallback?: () => JSX.Element): () => JSX.Element {
  const length = computed(() => {
    const value = count();
    return value > 0 ? Math.trunc(value) : 0;
  });
  return list(
    () => indexes(length()),
    (index) => map(index()),
    fallback,
  );
}
