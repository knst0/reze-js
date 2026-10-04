import { MathMLNamespace } from "../../../crates/reze_compiler/src/html-data.json";

interface TextNodePatch {
  readonly path: readonly (number | "c")[];
  readonly children: readonly (string | null)[];
}

export interface TextNodeTemplate {
  readonly source: string;
  readonly namespace: "" | "svg" | "math";
  readonly patches: readonly TextNodePatch[];
}

const roots = new WeakMap<TextNodeTemplate, Node>();

/** Clones a parsed template whose raw-text logical boundaries are installed once before cloning. */
export function templateWithTextNodes(definition: TextNodeTemplate): Node {
  let root = roots.get(definition);
  if (root === undefined) {
    if (definition.namespace === "math") {
      const container = document.createElementNS(MathMLNamespace, "template");
      container.innerHTML = definition.source;
      root = container.firstChild!;
    } else {
      const container = document.createElement("template");
      container.innerHTML = definition.source;
      root = container.content.firstChild!;
      if (definition.namespace === "svg") root = root.firstChild!;
    }
    for (const patch of definition.patches) {
      let parent = root;
      for (const step of patch.path) {
        parent = step === "c" ? (parent as HTMLTemplateElement).content : parent.childNodes[step]!;
      }
      for (const child of patch.children) {
        parent.appendChild(child === null ? document.createComment("") : document.createTextNode(child));
      }
    }
    roots.set(definition, root);
  }
  return root.cloneNode(true);
}
