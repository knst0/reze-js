const MathMLNamespace = "http://www.w3.org/1998/Math/MathML";

const htmlRoots = new Map<string, Node>();
const svgRoots = new Map<string, Node>();
const mathMLRoots = new Map<string, Node>();

function parseHTML(source: string): Node {
  const container = document.createElement("template");
  container.innerHTML = source;
  return container.content.firstChild!;
}

/** Parses `source` on the first call with it, keeps the root for the page's lifetime and returns a deep clone of it. */
export function template(source: string): Node {
  let node = htmlRoots.get(source);
  if (node === undefined) {
    htmlRoots.set(source, (node = parseHTML(source)));
  }
  return node.cloneNode(true);
}

/** {@link template} for an SVG root: `source` is that root preceded by `<svg>`. */
export function templateSVG(source: string): Node {
  let node = svgRoots.get(source);
  if (node === undefined) {
    svgRoots.set(source, (node = parseHTML(source).firstChild!));
  }
  return node.cloneNode(true);
}

/** {@link template} for a MathML root, parsed in the MathML namespace. */
export function templateMathML(source: string): Node {
  let node = mathMLRoots.get(source);
  if (node === undefined) {
    const container = document.createElementNS(MathMLNamespace, "template");
    container.innerHTML = source;
    mathMLRoots.set(source, (node = container.firstChild!));
  }
  return node.cloneNode(true);
}
