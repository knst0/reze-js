import { SVGElements } from "../../../../crates/reze_compiler/src/html-data.json";
import { element, elementMathML, elementSVG } from "../element";
import type { JSX } from "../jsx";
import { prepareDynamic } from "./flows";
import { claimRoot } from "./native";
import type { NamespaceKey, Site } from "./protocol";
import { preparingSession } from "./session";
import { queueSpread } from "./spread";

type Props = Record<string, unknown>;
type ElementComponent = (props: Props) => Element;
const elementTypes = new WeakMap<Site, Map<string, ElementComponent>>();

export function prepareElementType(site: Site, tag: string, namespace: NamespaceKey = ""): ElementComponent {
  let cache = elementTypes.get(site);
  if (cache === undefined) elementTypes.set(site, cache = new Map());
  const key = `${namespace}:${tag}`;
  let component = cache.get(key);
  if (component === undefined) {
    component = props => {
      if (preparingSession() === undefined) {
        return (namespace === "svg" ? elementSVG(tag) : namespace === "math" ? elementMathML(tag) : element(tag))(props);
      }
      const node = claimRoot(site, tag, namespace);
      queueSpread(node, site, props, namespace !== "");
      return node;
    };
    cache.set(key, component);
  }
  return component;
}

export function prepareDynamicElement(site: Site, source: () => string | ((props: Props) => JSX.Element) | null | undefined | false): (props: Props) => JSX.Element {
  return prepareDynamic(site, () => {
    const type = source();
    if (typeof type !== "string") return type;
    if (!type) return undefined;
    return prepareElementType(site, type, Object.hasOwn(SVGElements, type) ? "svg" : type === "math" ? "math" : "");
  });
}
