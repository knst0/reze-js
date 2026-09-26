const MathMLNamespace = "http://www.w3.org/1998/Math/MathML";

function parseHTML(html: string): Node {
  const container = document.createElement("template");
  container.innerHTML = html;
  return container.content.firstChild!;
}

function parseMathML(html: string): Node {
  const container = document.createElementNS(MathMLNamespace, "template");
  container.innerHTML = html;
  return container.firstChild!;
}

/** Parses `html` on the first call and returns a deep clone of its root on every call. */
export function template(html: string): () => Node {
  let node: Node | undefined;
  return () => (node ??= parseHTML(html)).cloneNode(true);
}

/** {@link template} for an SVG root: `html` is that root wrapped in `<svg>…</svg>`. */
export function templateSVG(html: string): () => Node {
  let node: Node | undefined;
  return () => (node ??= parseHTML(html).firstChild!).cloneNode(true);
}

/** {@link template} for a MathML root, parsed in the MathML namespace. */
export function templateMathML(html: string): () => Node {
  let node: Node | undefined;
  return () => (node ??= parseMathML(html)).cloneNode(true);
}
