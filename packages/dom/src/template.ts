import { MathMLNamespace } from "../../../crates/reze_compiler/src/html-data.json";

const htmlRoots = new Map<string, Node>();
const svgRoots = new Map<string, Node>();
const mathMLRoots = new Map<string, Node>();

/** Raised when the server DOM does not line up with the component's templates. */
export class AdoptionError extends Error {
  constructor(detail: string) {
    super(`[reze] island adoption failed: ${detail}`);
    this.name = "AdoptionError";
  }
}

interface Adoption {
  readonly nodes: readonly Node[];
  index: number;
}

let adoption: Adoption | undefined;

/** Claims the existing server nodes in render order until `endAdoption`. */
export function beginAdoption(nodes: readonly Node[]): Adoption {
  const state: Adoption = { nodes, index: 0 };
  adoption = state;
  return state;
}

export function endAdoption(): void {
  adoption = undefined;
}

function claim(source: string, roots: Map<string, Node>, parse: () => Node): Node | undefined {
  if (adoption === undefined) return undefined;
  const node = adoption.nodes[adoption.index];
  if (node === undefined) throw new AdoptionError("the component rendered more roots than the server sent");
  let shape = roots.get(source);
  if (shape === undefined) roots.set(source, (shape = parse()));
  if (node.nodeType !== shape.nodeType || (node as Element).nodeName !== (shape as Element).nodeName) {
    throw new AdoptionError(`expected <${(shape as Element).nodeName?.toLowerCase()}> at root ${adoption.index}`);
  }
  adoption.index++;
  return node;
}

function parseHTML(source: string): Node {
  const container = document.createElement("template");
  container.innerHTML = source;
  return container.content.firstChild!;
}

/** Parses `source` on the first call with it, keeps the root for the page's lifetime and returns a deep clone of it. */
export function template(source: string): Node {
  const claimed = claim(source, htmlRoots, () => parseHTML(source));
  if (claimed !== undefined) return claimed;
  let node = htmlRoots.get(source);
  if (node === undefined) {
    htmlRoots.set(source, (node = parseHTML(source)));
  }
  return node.cloneNode(true);
}

/** {@link template} for an SVG root: `source` is that root preceded by `<svg>`. */
export function templateSVG(source: string): Node {
  const claimed = claim(source, svgRoots, () => parseHTML(source).firstChild!);
  if (claimed !== undefined) return claimed;
  let node = svgRoots.get(source);
  if (node === undefined) {
    svgRoots.set(source, (node = parseHTML(source).firstChild!));
  }
  return node.cloneNode(true);
}

/** {@link template} for a MathML root, parsed in the MathML namespace. */
export function templateMathML(source: string): Node {
  const claimed = claim(source, mathMLRoots, () => {
    const container = document.createElementNS(MathMLNamespace, "template");
    container.innerHTML = source;
    return container.firstChild!;
  });
  if (claimed !== undefined) return claimed;
  let node = mathMLRoots.get(source);
  if (node === undefined) {
    const container = document.createElementNS(MathMLNamespace, "template");
    container.innerHTML = source;
    mathMLRoots.set(source, (node = container.firstChild!));
  }
  return node.cloneNode(true);
}
