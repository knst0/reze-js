import { MathMLNamespace, SVGNamespace } from "../../../crates/reze_compiler/src/html-data.json";
import { spread } from "./spread";

type Props = Record<string, unknown>;
type ElementComponent = (props: Props) => Element;

const htmlComponents = new Map<string, ElementComponent>();
const svgComponents = new Map<string, ElementComponent>();
const mathMLComponents = new Map<string, ElementComponent>();

function elementComponent(components: Map<string, ElementComponent>, tag: string, namespace?: string): ElementComponent {
  let component = components.get(tag);
  if (component === undefined) {
    component = (props) => {
      const node = namespace === undefined ? document.createElement(tag) : document.createElementNS(namespace, tag);
      spread(node, props, namespace !== undefined);
      return node;
    };
    components.set(tag, component);
  }
  return component;
}

/**
 * The component rendering an HTML `tag` with its props, which the compiler puts in place of a tag name a `dynamic`
 * source returns. One per tag, so a source returning the same tag again keeps what it rendered.
 */
export function element(tag: string): ElementComponent {
  return elementComponent(htmlComponents, tag);
}

/** {@link element} for an SVG `tag`. */
export function elementSVG(tag: string): ElementComponent {
  return elementComponent(svgComponents, tag, SVGNamespace);
}

/** {@link element} for a MathML `tag`. */
export function elementMathML(tag: string): ElementComponent {
  return elementComponent(mathMLComponents, tag, MathMLNamespace);
}
