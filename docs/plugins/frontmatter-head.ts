import type { Root, RootContent } from "mdast";
import type { Plugin } from "unified";
import { parse as parseYaml } from "yaml";

const HeadBinding = "FrontmatterHead";

function jsxElement(name: string, attributes: Record<string, string>, children: unknown[]): RootContent {
  return {
    type: "mdxJsxFlowElement",
    name,
    attributes: Object.entries(attributes).map(([key, value]) => ({ type: "mdxJsxAttribute", name: key, value })),
    children,
    data: { _mdxExplicitJsx: true },
  } as RootContent;
}

const headImport = {
  type: "mdxjsEsm",
  value: `import { Head as ${HeadBinding} } from "reze-js";`,
  data: {
    estree: {
      type: "Program",
      sourceType: "module",
      body: [
        {
          type: "ImportDeclaration",
          specifiers: [
            {
              type: "ImportSpecifier",
              imported: { type: "Identifier", name: "Head" },
              local: { type: "Identifier", name: HeadBinding },
            },
          ],
          source: { type: "Literal", value: "reze-js", raw: '"reze-js"' },
        },
      ],
    },
  },
} as unknown as RootContent;

const frontmatterHead: Plugin<[], Root> = () => (ast, file) => {
  const invalid = `[docs] ${file.path ?? "page"}: frontmatter requires non-empty title and description strings`;
  const frontmatter = ast.children.find((child) => child.type === "yaml");
  if (frontmatter === undefined || typeof frontmatter.value !== "string") throw new Error(invalid);
  const fields = parseYaml(frontmatter.value) as Record<string, unknown> | null;
  const title = fields?.title;
  const description = fields?.description;
  if (typeof title !== "string" || title.trim() === "" || typeof description !== "string" || description.trim() === "") {
    throw new Error(invalid);
  }
  ast.children.unshift(
    headImport,
    jsxElement(HeadBinding, {}, [
      jsxElement("title", {}, [{ type: "text", value: title }]),
      jsxElement("meta", { name: "description", content: description }, []),
    ]),
  );
};

export default frontmatterHead;
