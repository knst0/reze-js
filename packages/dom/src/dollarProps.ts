import type { Props, SplitProps } from "./props";

type PropsSyntax = {
  /**
   * Merges sources left to right: the last defined value wins, `undefined` counts as absent.
   * Dissolves into an object literal when every source is an object literal of defined values
   * with static keys, and compiles to `mergeProps` otherwise.
   */
  merge(...sources: unknown[]): Props;
  /**
   * Splits `props` into lazy views, one per key group plus one with the remaining keys, like
   * `splitProps`. Dissolves into literals when `props` is an object literal and every group is
   * an array literal of strings, and compiles to `splitProps` otherwise.
   */
  splitByGroups<T extends object, const K extends readonly (readonly (keyof T)[])[]>(props: T, ...groups: K): SplitProps<T, K>;
  /**
   * The rest of `props` without `keys`, like `omitProps`. Dissolves into a literal when `props`
   * is an object literal and every key is a string literal, and compiles to `omitProps`
   * otherwise.
   */
  omit<T extends object, const K extends readonly (keyof T)[]>(props: T, ...keys: K): Omit<T, K[number]>;
};

/**
 * Compiler syntax for props handling: import it from `reze-js`, call one of the three methods,
 * and the compiler rewrites the call. A local `$props` is an ordinary value and is left alone;
 * any other use of the import is a `PROPS_AS_VALUE` error.
 */
export const $props: PropsSyntax = {
  merge(): never {
    throw new Error("`$props.merge()` requires the reze compiler");
  },
  splitByGroups(): never {
    throw new Error("`$props.splitByGroups()` requires the reze compiler");
  },
  omit(): never {
    throw new Error("`$props.omit()` requires the reze compiler");
  },
};
