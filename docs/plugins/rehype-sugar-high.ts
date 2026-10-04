import { generate, parse, type GeneratedLine } from "sugar-high/core";
import { lang, languages } from "sugar-high/lang";

type TextNode = { type: "text"; value: string };

type HastNode = {
  type: string;
  tagName?: string;
  properties?: Record<string, unknown>;
  children?: Array<HastNode>;
  value?: string;
};

function innerText(node: HastNode): string {
  if (node.type === "text") return node.value ?? "";
  let out = "";
  for (const child of node.children ?? []) out += innerText(child);
  return out;
}

function languageOf(code: HastNode): string | undefined {
  const className = code.properties?.["className"];
  if (!Array.isArray(className)) return undefined;
  const found = className.find((name): name is string => typeof name === "string" && name.startsWith("language-"));
  return found?.slice("language-".length);
}

function toText(value: string): TextNode {
  return { type: "text", value };
}

function toLine(line: GeneratedLine, index: number): HastNode {
  return {
    type: "element",
    tagName: "span",
    properties: { "data-sh-line": index + 1 },
    children: line.children.map((token) => ({
      type: "element",
      tagName: "span",
      properties: { "data-sh-token": token.tokenType },
      children: token.children.map((text) => toText(text.value)),
    })),
  };
}

function highlight(code: HastNode, language: string): void {
  const canonical = lang(language) ?? "javascript";
  const config = languages.find((entry) => entry.id === canonical)?.config;
  const lines = generate(parse(innerText(code), config));
  const children: Array<HastNode> = [];
  for (const [index, line] of lines.entries()) {
    if (index > 0) children.push(toText("\n"));
    children.push(toLine(line, index));
  }
  code.properties = { "data-sh-language": canonical };
  code.children = children;
}

function visit(node: HastNode): void {
  const children = node.children;
  if (!Array.isArray(children)) return;
  for (const child of children) {
    if (child.type === "element" && child.tagName === "pre" && child.children?.length === 1) {
      const [code] = child.children;
      if (code?.type === "element" && code.tagName === "code") {
        const language = languageOf(code);
        if (language !== undefined) highlight(code, language);
      }
    }
    visit(child);
  }
}

export default function rehypeSugarHigh() {
  return (tree: HastNode) => {
    visit(tree);
  };
}
