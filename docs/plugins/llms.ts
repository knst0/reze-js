import { readdir, readFile } from "node:fs/promises";
import { posix, resolve } from "node:path";

import { scanRoutes, type FileRoute } from "@rezejs/router/fs";
import type { Root, RootContent } from "mdast";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMdx from "remark-mdx";
import remarkParse from "remark-parse";
import remarkStringify from "remark-stringify";
import { unified } from "unified";
import type { Plugin } from "vite";
import { parse } from "yaml";

const parser = unified().use(remarkParse).use(remarkFrontmatter).use(remarkGfm).use(remarkMdx);
const writer = unified().use(remarkStringify).use(remarkGfm);

function markdownPath(route: string): string {
  return route === "/" ? "/index.md" : `${route}.md`;
}

function cleanChildren(children: RootContent[], route: string, pages: Set<string>, base: string): RootContent[] {
  return children.flatMap((node): RootContent[] => {
    if (node.type === "yaml" || node.type === "mdxjsEsm" || node.type === "mdxFlowExpression" || node.type === "mdxTextExpression")
      return [];
    if ("children" in node) node.children = cleanChildren(node.children as RootContent[], route, pages, base) as typeof node.children;
    if (node.type === "mdxJsxFlowElement" || node.type === "mdxJsxTextElement") return node.children as RootContent[];
    if ((node.type === "link" || node.type === "definition") && !/^(?:[a-z][a-z\d+.-]*:|\/\/|#)/i.test(node.url)) {
      const url = new URL(node.url, `https://docs.invalid${route}`);
      const pathname = url.pathname.replace(/\/$/, "") || "/";
      if (pages.has(pathname)) node.url = `${base}${markdownPath(pathname).slice(1)}${url.search}${url.hash}`;
    }
    return [node];
  });
}

async function artifacts(directory: string, base: string): Promise<Map<string, string>> {
  const files = (await readdir(directory, { recursive: true })).map((file) => file.replaceAll("\\", "/"));
  const routes: FileRoute[] = [];
  function collect(tree: FileRoute[]): void {
    for (const route of tree) {
      if (route.file.endsWith(".mdx") && route.children.length === 0) {
        if (/\/(?:[:*])/.test(route.fullPath)) throw new Error(`Cannot generate static Markdown for ${route.file}`);
        routes.push(route);
      }
      collect(route.children);
    }
  }
  collect(scanRoutes(files, { extensions: [".tsx", ".ts", ".mdx"] }));
  const paths = new Set(routes.map((route) => route.fullPath));
  const output = new Map<string, string>();
  const index: Root = {
    type: "root",
    children: [
      { type: "heading", depth: 1, children: [{ type: "text", value: "Reze" }] },
      {
        type: "blockquote",
        children: [
          {
            type: "paragraph",
            children: [
              { type: "text", value: "Compiler-driven UI framework with fine-grained reactivity. Documentation and API reference." },
            ],
          },
        ],
      },
      { type: "heading", depth: 2, children: [{ type: "text", value: "Documentation" }] },
    ],
  };
  const list: RootContent & { type: "list" } = { type: "list", ordered: false, spread: false, children: [] };
  for (const route of routes) {
    const file = resolve(directory, route.file);
    const tree = parser.parse(await readFile(file, "utf8"));
    const yaml = tree.children.find((node) => node.type === "yaml");
    const meta = yaml ? parse(yaml.value) : undefined;
    if (!meta || typeof meta.title !== "string" || !meta.title.trim() || typeof meta.description !== "string" || !meta.description.trim()) {
      throw new Error(`${file}: frontmatter requires non-empty title and description strings`);
    }
    tree.children = cleanChildren(tree.children, route.fullPath, paths, base);
    const filename = markdownPath(route.fullPath).slice(1);
    output.set(filename, writer.stringify(tree));
    list.children.push({
      type: "listItem",
      spread: false,
      children: [
        {
          type: "paragraph",
          children: [
            { type: "link", url: `${base}${filename}`, children: [{ type: "text", value: meta.title }] },
            { type: "text", value: `: ${meta.description.replace(/\s+/g, " ").trim()}` },
          ],
        },
      ],
    });
  }
  index.children.push(list);
  output.set("llms.txt", writer.stringify(index));
  return output;
}

export default function llms(): Plugin {
  let directory: string;
  let base: string;
  return {
    name: "docs-llms",
    configResolved(config) {
      directory = resolve(config.root, "src/routes");
      base = config.base === "./" || config.base === "" ? "/" : new URL(config.base, "https://docs.invalid").pathname;
    },
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        if (req.method !== "GET" && req.method !== "HEAD") return next();
        const pathname = new URL(req.url ?? "/", "http://localhost").pathname;
        if (pathname !== posix.join(base, "llms.txt") && !pathname.endsWith(".md")) return next();
        if (!pathname.startsWith(base)) return next();
        try {
          const filename = decodeURIComponent(pathname.slice(base.length));
          const content = (await artifacts(directory, base)).get(filename);
          if (content === undefined) {
            res.statusCode = 404;
            res.end("Not found\n");
            return;
          }
          res.setHeader("Content-Type", filename === "llms.txt" ? "text/plain; charset=utf-8" : "text/markdown; charset=utf-8");
          res.end(req.method === "HEAD" ? undefined : content);
        } catch (error) {
          next(error);
        }
      });
    },
    async generateBundle() {
      if (this.environment?.name !== "client") return;
      for (const [fileName, source] of await artifacts(directory, base)) {
        this.emitFile({ type: "asset", fileName, source });
      }
    },
  };
}
