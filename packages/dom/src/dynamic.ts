import { computed } from "@rezejs/signals";

import { createComponent } from "./component";
import type { JSX } from "./jsx";

type Props = Record<string, unknown>;

export type PropsOf<T> = T extends keyof JSX.IntrinsicElements
  ? JSX.IntrinsicElements[T]
  : T extends (props: infer P) => unknown
    ? P
    : never;

/**
 * A component that renders what `source()` returns: a component, or nothing when falsy. It switches when `source()`
 * changes to a different value, disposing what it rendered before; props stay live either way. Tag names work only as
 * string literals `source` returns, which the compiler turns into components; use `dynamicElement` for tag names chosen
 * at runtime. Create it once, outside any component, since every call is a new component. A component kept in a signal
 * is set with `setter(() => Component)`, as a bare function is an updater.
 */
export function dynamic<T extends JSX.ElementType>(source: () => T | null | undefined | false): (props: PropsOf<T>) => JSX.Element;
export function dynamic(source: () => JSX.ElementType | null | undefined | false): (props: Props) => JSX.Element {
  return (props) => {
    const type = computed(source);
    return () => {
      const current = type();
      if (!current) {
        return undefined;
      }
      if (process.env.NODE_ENV !== "production" && typeof current === "string") {
        throw new Error(
          `[reze] dynamic: the tag name "${current}" is not a string literal its source returns; use dynamicElement for tag names chosen at runtime`,
        );
      }
      return createComponent(current as (props: Props) => JSX.Element, props);
    };
  };
}
