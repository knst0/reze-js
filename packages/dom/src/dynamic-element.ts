import { SVGElements } from "../../../crates/reze_compiler/src/html-data.json";

import { dynamic, type PropsOf } from "./dynamic";
import { element, elementMathML, elementSVG } from "./element";
import type { JSX } from "./jsx";

/**
 * `dynamic` that also renders tag names chosen at runtime, such as `props.as`, in the namespace of the tag: SVG for SVG
 * element names, MathML for `math`, HTML otherwise.
 */
export function dynamicElement<T extends JSX.ElementType>(source: () => T | null | undefined | false): (props: PropsOf<T>) => JSX.Element;
export function dynamicElement(source: () => JSX.ElementType | null | undefined | false): (props: Record<string, unknown>) => JSX.Element {
  return dynamic(() => {
    const type = source();
    if (!type || typeof type !== "string") {
      return type;
    }
    if (Object.hasOwn(SVGElements, type)) {
      return elementSVG(type);
    }
    return type === "math" ? elementMathML(type) : element(type);
  });
}
