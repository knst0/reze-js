/** Resolves a built view eagerly: calls thunks until a non-function remains, flattens arrays resolving each item. */
export function resolve(value: unknown): unknown {
  while (typeof value === "function") {
    value = (value as () => unknown)();
  }
  if (Array.isArray(value)) {
    const out: unknown[] = [];
    for (const item of value) {
      const resolved = resolve(item);
      if (Array.isArray(resolved)) {
        out.push(...resolved);
      } else {
        out.push(resolved);
      }
    }
    return out;
  }
  return value;
}
