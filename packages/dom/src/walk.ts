/** `node.firstChild` of a template clone, which always has the child the compiler walks to. */
export function child(node: Node): Node {
  return node.firstChild!;
}

/** `node.nextSibling` of a template clone, which always has the sibling the compiler walks to. */
export function next(node: Node): Node {
  return node.nextSibling!;
}
