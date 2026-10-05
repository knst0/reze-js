import { setAttribute } from "./properties";
import { serializeNodes } from "./serialize";
import { createElement, type HtmlNode } from "./tree";

function transportNode(node: HtmlNode): HtmlNode {
  if (node.kind === "range") return { ...node, children: node.children.map(transportNode) };
  if (node.kind !== "element" || node.ns === "" || node.tag === (node.ns === "svg" ? "svg" : "math")) return node;
  const wrapper = createElement(node.ns === "svg" ? "svg" : "math", node.ns);
  setAttribute(wrapper, "data-reze-context", node.ns);
  wrapper.children.push(node);
  return wrapper;
}

export function serializePortalNodes(nodes: readonly HtmlNode[], ownerTokens?: ReadonlyMap<string, string>): string {
  return serializeNodes(nodes.map(transportNode), ownerTokens);
}
