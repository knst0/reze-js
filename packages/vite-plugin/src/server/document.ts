import { existsSync, readFileSync } from "node:fs";

import { compileDocument } from "@rezejs/compiler";
import type { DocumentImport } from "@rezejs/compiler";

import { ClientRequest } from "./adapter";
import { rootRenderedDocument, withBootstrap } from "./template";

export interface AppDocument {
  html: string;
  rootId: string;
  /** Files the page was compiled from; `undefined` when the document module ran to render it. */
  files: readonly string[] | undefined;
}

export type ImportResolver = (specifier: string, importer: string) => Promise<string | undefined>;

export type DocumentAnalysis = { status: "static"; document: AppDocument } | { status: "render"; reasons: string };

const DefaultDocumentSource = `export default function Document(props) {
  return (
    <html lang="en">
      <head>
        <meta charset="utf-8" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
      </head>
      <body>
        <div id="app">{props.children}</div>
      </body>
    </html>
  );
}
`;

const SourceFile = /\.(?:[cm]?[jt]sx?)$/;
const QueryOrHash = /[?#].*$/;

function readableSource(resolved: string | undefined): string | undefined {
  if (resolved === undefined) return undefined;
  const file = resolved.replace(QueryOrHash, "");
  return SourceFile.test(file) && !file.endsWith(".d.ts") && existsSync(file) ? file : undefined;
}

function defineCode(value: unknown): string {
  if (value === undefined) return "undefined";
  return typeof value === "string" ? value : JSON.stringify(value);
}

export async function analyzeDocument(
  file: string,
  resolveImport: ImportResolver,
  define: Record<string, unknown> | undefined,
  bootstrapSrc: string | null = ClientRequest,
): Promise<DocumentAnalysis> {
  const source = existsSync(file) ? readFileSync(file, "utf8") : DefaultDocumentSource;
  const imports: DocumentImport[] = [];
  const sources: Record<string, string> = {};
  const defines = Object.fromEntries(Object.entries(define ?? {}).map(([key, value]) => [key, defineCode(value)]));
  for (;;) {
    const result = compileDocument(source, file, { imports, sources, defines });
    if (result.requests.length > 0) {
      const resolved = await Promise.all(
        result.requests.map(async (request) => readableSource(await resolveImport(request.specifier, request.importer))),
      );
      result.requests.forEach((request, index) => {
        const target = resolved[index];
        if (target !== undefined) sources[target] ??= readFileSync(target, "utf8");
        imports.push({ ...request, file: target });
      });
      continue;
    }
    const rendered = result.diagnostics.map((d) => `${d.rendered.trimEnd()}\n  repair guide: ${d.docs}`).join("\n\n");
    if (result.render) return { status: "render", reasons: rendered };
    if (result.html === undefined || result.rootId === undefined) throw new Error(rendered);
    const html = bootstrapSrc === null ? result.html : withBootstrap(result.html, bootstrapSrc);
    return { status: "static", document: { html, rootId: result.rootId, files: result.files } };
  }
}

export function renderedDocument(rendered: string, file: string): AppDocument {
  const { html, rootId } = rootRenderedDocument(rendered, file);
  return { html: withBootstrap(html, ClientRequest), rootId, files: undefined };
}
