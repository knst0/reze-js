import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

import GithubSlugger from "github-slugger";
import type { Code, Link, Nodes, Root } from "mdast";
import { toString } from "mdast-util-to-string";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMdx from "remark-mdx";
import remarkParse from "remark-parse";
import { unified } from "unified";

const parser = unified().use(remarkParse).use(remarkFrontmatter).use(remarkGfm).use(remarkMdx);
const compiledLanguages = new Set(["ts", "tsx"]);

export interface Page {
  /** Path under the routes directory, without `.mdx`. */
  name: string;
  /** Site path the page is served at. */
  route: string;
  tree: Root;
}

export interface Example {
  /** `<page>.mdx:<line>` of the opening fence. */
  id: string;
  page: string;
  /** Path inside the page's project: `file=<path>` from the fence meta, else `example-<line>.<lang>`. */
  file: string;
  source: string;
  /** Diagnostic codes the sample must produce, from `expect=CODE[,CODE]` in the fence meta. */
  expectedCodes: string[];
}

function collect<T extends Nodes>(node: Nodes, type: T["type"], out: T[]): void {
  if (node.type === type) out.push(node as T);
  if ("children" in node) for (const child of node.children) collect(child, type, out);
}

function metaValue(tokens: string[], key: string): string | undefined {
  return tokens.find((token) => token.startsWith(`${key}=`))?.slice(key.length + 1);
}

/** Every `.mdx` page under `routesDir`, sorted by name. */
export async function readPages(routesDir: string): Promise<Page[]> {
  const files = (await readdir(routesDir, { recursive: true }))
    .map((file) => file.replaceAll("\\", "/"))
    .filter((file) => file.endsWith(".mdx"))
    .sort();
  return Promise.all(
    files.map(async (file) => {
      const name = file.slice(0, -".mdx".length);
      const route = `/${name.replace(/(?:^|\/)index$/, "")}`;
      return { name, route, tree: parser.parse(await readFile(join(routesDir, file), "utf8")) };
    }),
  );
}

/** `ts` and `tsx` samples in source order. A fence whose meta holds `fragment` is not a module and is skipped. */
export function collectExamples(page: Page): Example[] {
  const blocks: Code[] = [];
  collect(page.tree, "code", blocks);
  const examples: Example[] = [];
  for (const block of blocks) {
    if (block.lang === null || block.lang === undefined || !compiledLanguages.has(block.lang)) continue;
    const tokens = (block.meta ?? "").split(/\s+/).filter((token) => token !== "");
    if (tokens.includes("fragment")) continue;
    const line = block.position?.start.line ?? 0;
    const expected = metaValue(tokens, "expect");
    examples.push({
      id: `${page.name}.mdx:${line}`,
      page: page.name,
      file: metaValue(tokens, "file") ?? `example-${line}.${block.lang}`,
      source: block.value,
      expectedCodes: expected === undefined ? [] : expected.split(",").sort(),
    });
  }
  return examples;
}

/** Heading anchors in document order, as `rehype-slug` derives them. */
export function headingAnchors(page: Page): Set<string> {
  const slugger = new GithubSlugger();
  const headings: Array<Nodes & { type: "heading" }> = [];
  collect(page.tree, "heading", headings);
  return new Set(headings.map((heading) => slugger.slug(toString(heading))));
}

/** Link targets that start with `/` or `#`; `#` targets are resolved against the page itself. */
export function internalLinks(page: Page): string[] {
  const links: Link[] = [];
  collect(page.tree, "link", links);
  return links
    .map((link) => link.url)
    .filter((url) => url.startsWith("/") || url.startsWith("#"))
    .map((url) => (url.startsWith("#") ? `${page.route}${url}` : url));
}
