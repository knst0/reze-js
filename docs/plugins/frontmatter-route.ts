import type { Root } from "mdast";
import type { Plugin } from "unified";
import { parse as parseYaml } from "yaml";

interface EstreeNode {
  type: string;
  name?: unknown;
  value?: unknown;
  start?: number;
  end?: number;
  declaration?: EstreeNode | null;
  declarations?: EstreeNode[];
  id?: EstreeNode;
  init?: EstreeNode | null;
  key?: EstreeNode;
  kind?: string;
  computed?: boolean;
  properties?: EstreeNode[];
  argument?: EstreeNode;
  specifiers?: EstreeNode[];
  local?: EstreeNode;
  exported?: EstreeNode;
  imported?: EstreeNode;
  source?: EstreeNode | null;
  body?: EstreeNode[];
  comments?: unknown[];
}

interface RawMdxNode {
  type: string;
  value: string;
  data?: { estree?: { type: string; body?: EstreeNode[] } };
}

function keyName(node: EstreeNode | undefined): string | undefined {
  if (node === undefined) return undefined;
  if (node.type === "Identifier" && typeof node.name === "string") return node.name;
  if (node.type === "Literal" && typeof node.value === "string") return node.value;
  return undefined;
}

function isObjectLiteral(node: EstreeNode | undefined | null): node is EstreeNode {
  return node !== undefined && node !== null && node.type === "ObjectExpression" && Array.isArray(node.properties);
}

function isRouteDeclarator(node: EstreeNode | undefined): node is EstreeNode {
  return node !== undefined && node.type === "VariableDeclarator" && node.id?.type === "Identifier" && node.id.name === "route";
}

const frontmatterRoute: Plugin<[], Root> = () => (ast, file) => {
  const id = file.path ?? "route";
  const invalid = `[docs] ${id}: frontmatter requires non-empty title and description strings`;
  const literal = `[docs] ${id}: route export must be a plain object literal so frontmatter metadata can merge into it`;
  const owned = `[docs] ${id}: route export already defines "meta"; page metadata comes from frontmatter, so remove "meta" from the route export`;
  const frontmatter = (ast.children as unknown as Array<{ type: string; value: unknown }>).find(
    (child) => child.type === "yaml" || child.type === "toml",
  );
  if (frontmatter === undefined || frontmatter.type !== "yaml" || typeof frontmatter.value !== "string") {
    throw new Error(invalid);
  }
  const fields = parseYaml(frontmatter.value) as Record<string, unknown>;
  const title = fields?.title;
  const description = fields?.description;
  if (
    typeof fields !== "object" ||
    fields === null ||
    typeof title !== "string" ||
    title.trim() === "" ||
    typeof description !== "string" ||
    description.trim() === ""
  ) {
    throw new Error(invalid);
  }

  const metaSource = `meta: {title: ${JSON.stringify(title)}, description: ${JSON.stringify(description)}}`;
  const metaProperty: EstreeNode = {
    type: "Property",
    kind: "init",
    computed: false,
    key: { type: "Identifier", name: "meta" },
    value: {
      type: "ObjectExpression",
      properties: [
        {
          type: "Property",
          kind: "init",
          computed: false,
          key: { type: "Identifier", name: "title" },
          value: { type: "Literal", value: title },
        },
        {
          type: "Property",
          kind: "init",
          computed: false,
          key: { type: "Identifier", name: "description" },
          value: { type: "Literal", value: description },
        },
      ],
    },
  };

  const nodes = ast.children as unknown as RawMdxNode[];
  let declared: { node: RawMdxNode; init: EstreeNode | null } | undefined;
  let imported = false;
  for (const node of nodes) {
    if (node.type !== "mdxjsEsm") continue;
    for (const statement of node.data?.estree?.body ?? []) {
      if (statement.type === "VariableDeclaration") {
        for (const declarator of statement.declarations ?? []) {
          if (isRouteDeclarator(declarator)) declared = { node, init: declarator.init ?? null };
        }
      } else if (statement.type === "ImportDeclaration") {
        for (const specifier of statement.specifiers ?? []) {
          if (keyName(specifier.local) === "route" || keyName(specifier.imported as EstreeNode | undefined) === "route") {
            imported = true;
          }
        }
      }
    }
  }

  const mergeTargets: Array<{ node: RawMdxNode; init: EstreeNode | null }> = [];
  for (const node of nodes) {
    if (node.type !== "mdxjsEsm") continue;
    for (const statement of node.data?.estree?.body ?? []) {
      if (statement.type === "ExportNamedDeclaration" && statement.declaration?.type === "VariableDeclaration") {
        for (const declarator of statement.declaration.declarations ?? []) {
          if (isRouteDeclarator(declarator)) mergeTargets.push({ node, init: declarator.init ?? null });
        }
      } else if (statement.type === "ExportNamedDeclaration" && (statement.declaration === undefined || statement.declaration === null)) {
        for (const specifier of statement.specifiers ?? []) {
          if (specifier.type === "ExportSpecifier" && keyName(specifier.exported) === "route") {
            if (keyName(specifier.local) !== "route" || imported || declared === undefined) {
              throw new Error(literal);
            }
            mergeTargets.push(declared);
          }
        }
      }
    }
  }

  if (mergeTargets.length === 0) {
    if (declared !== undefined || imported) {
      throw new Error(literal);
    }
    nodes.push({
      type: "mdxjsEsm",
      value: `export const route = {${metaSource}};`,
      data: {
        estree: {
          type: "Program",
          body: [
            {
              type: "ExportNamedDeclaration",
              declaration: {
                type: "VariableDeclaration",
                kind: "const",
                declarations: [
                  {
                    type: "VariableDeclarator",
                    id: { type: "Identifier", name: "route" },
                    init: { type: "ObjectExpression", properties: [metaProperty] },
                  },
                ],
              },
              specifiers: [],
              source: null,
            },
          ],
        },
      },
    });
    return;
  }

  for (const { node, init } of mergeTargets) {
    if (!isObjectLiteral(init)) {
      throw new Error(literal);
    }
    for (const property of init.properties ?? []) {
      if (property.type === "SpreadElement") {
        throw new Error(owned);
      }
      if (property.type === "Property" && keyName(property.key) === "meta") {
        throw new Error(owned);
      }
    }
    const empty = (init.properties ?? []).length === 0;
    if (
      typeof init.start === "number" &&
      typeof init.end === "number" &&
      node.value.slice(init.start, init.start + 1) === "{" &&
      node.value.slice(init.end - 1, init.end) === "}"
    ) {
      node.value = `${node.value.slice(0, init.end - 1)}${empty ? "" : ","}${metaSource}${node.value.slice(init.end - 1)}`;
    }
    (init.properties ??= []).push(metaProperty);
  }
};

export default frontmatterRoute;
