export type Props = Record<string, unknown>;

/**
 * Merges props objects lazily: each key reads the last source holding a non-`undefined` value, so getters stay
 * reactive. Function sources (dynamic spreads) are called on every access and may change the key set.
 */
export function mergeProps(...sources: unknown[]): Props {
  for (let i = 0; i < sources.length; i++) {
    if (typeof sources[i] === "function") {
      return mergeDynamic(sources);
    }
  }
  const merged: Props = {};
  for (let i = 0; i < sources.length; i++) {
    const source = sources[i];
    if (typeof source !== "object" || source === null) {
      continue;
    }
    for (const key in source) {
      if (!Object.hasOwn(merged, key)) {
        Object.defineProperty(merged, key, { configurable: true, enumerable: true, get: () => readLast(sources, key) });
      }
    }
  }
  return merged;
}

function readLast(sources: readonly unknown[], key: PropertyKey): unknown {
  for (let i = sources.length; i-- > 0;) {
    const source = resolveSource(sources[i]);
    if (source !== undefined) {
      const value = source[key];
      if (value !== undefined) {
        return value;
      }
    }
  }
  return undefined;
}

function resolveSource(source: unknown): Record<PropertyKey, unknown> | undefined {
  const resolved = typeof source === "function" ? source() : source;
  return typeof resolved === "object" && resolved !== null ? resolved : undefined;
}

function mergeDynamic(sources: readonly unknown[]): Props {
  return new Proxy<Props>(
    {},
    {
      get: (_, key) => readLast(sources, key),
      has: (_, key) => {
        for (let i = 0; i < sources.length; i++) {
          const source = resolveSource(sources[i]);
          if (source !== undefined && key in source) {
            return true;
          }
        }
        return false;
      },
      ownKeys: () => {
        const keys = new Set<string>();
        for (let i = 0; i < sources.length; i++) {
          const source = resolveSource(sources[i]);
          if (source !== undefined) {
            for (const key of Object.keys(source)) {
              keys.add(key);
            }
          }
        }
        return [...keys];
      },
      getOwnPropertyDescriptor: (_, key) => ({ configurable: true, enumerable: true, get: () => readLast(sources, key) }),
    },
  );
}

export type SplitProps<T, K extends readonly (readonly PropertyKey[])[]> = [
  ...{ [I in keyof K]: Pick<T, Extract<K[I][number], keyof T>> },
  Omit<T, K[number][number]>,
];

/** Splits `props` into lazy views, one per key group plus one with the remaining keys; getters stay reactive. */
export function splitProps<T extends object, const K extends readonly (readonly (keyof T)[])[]>(props: T, ...groups: K): SplitProps<T, K> {
  const views: Props[] = [];
  for (let i = 0; i <= groups.length; i++) {
    views.push({});
  }
  for (const key of Object.keys(props)) {
    let group = 0;
    while (group < groups.length && !groups[group]!.includes(key as keyof T)) {
      group++;
    }
    Object.defineProperty(views[group], key, {
      configurable: true,
      enumerable: true,
      get: () => (props as Props)[key],
    });
  }
  return views as unknown as SplitProps<T, K>;
}

/**
 * The rest of `props` without `keys`: a lazy view like the last result of `splitProps`, so
 * getters stay reactive. The key set is fixed when called.
 */
export function omitProps(props: Props, ...keys: readonly PropertyKey[]): Props {
  const rest: Props = {};
  for (const key of Object.keys(props)) {
    if (!(keys as readonly unknown[]).includes(key)) {
      Object.defineProperty(rest, key, {
        configurable: true,
        enumerable: true,
        get: () => (props as Props)[key],
      });
    }
  }
  return rest;
}
