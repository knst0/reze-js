import { dynamic, type PropsOf } from "./dynamic";
import { element, elementMathML, elementSVG } from "./element";
import type { JSX } from "./jsx";

/** `is_svg_element` of the compiler plus `svg`; names shared with HTML (`a`, `script`, `style`, `title`) stay HTML. */
const SVGElements: Record<string, true> = {
  svg: true,
  animate: true,
  animateMotion: true,
  animateTransform: true,
  circle: true,
  clipPath: true,
  cursor: true,
  defs: true,
  desc: true,
  discard: true,
  ellipse: true,
  feBlend: true,
  feColorMatrix: true,
  feComponentTransfer: true,
  feComposite: true,
  feConvolveMatrix: true,
  feDiffuseLighting: true,
  feDisplacementMap: true,
  feDistantLight: true,
  feDropShadow: true,
  feFlood: true,
  feFuncA: true,
  feFuncB: true,
  feFuncG: true,
  feFuncR: true,
  feGaussianBlur: true,
  feImage: true,
  feMerge: true,
  feMergeNode: true,
  feMorphology: true,
  feOffset: true,
  fePointLight: true,
  feSpecularLighting: true,
  feSpotLight: true,
  feTile: true,
  feTurbulence: true,
  filter: true,
  foreignObject: true,
  g: true,
  hatch: true,
  hatchpath: true,
  image: true,
  line: true,
  linearGradient: true,
  marker: true,
  mask: true,
  metadata: true,
  mpath: true,
  path: true,
  pattern: true,
  polygon: true,
  polyline: true,
  radialGradient: true,
  rect: true,
  set: true,
  solidcolor: true,
  stop: true,
  switch: true,
  symbol: true,
  text: true,
  textPath: true,
  tspan: true,
  use: true,
  view: true,
};

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
