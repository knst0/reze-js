import type { IncomingMessage, ServerResponse } from "node:http";

import type { ClientModules } from "reze-js/internal/html";
import { createNodeListener } from "reze-js/server";
import { isRunnableDevEnvironment } from "vite";
import type { Connect, EnvironmentModuleGraph, EnvironmentModuleNode, ViteDevServer } from "vite";

import { HtmlEnv, ServerEntryId } from "./adapter";
import type { AppDocument } from "./document";
import type { TemplateHead, TemplateParts } from "./template";
import { buildErrorPage, prepareTemplate } from "./template";

export interface DevDocumentOptions {
  document: () => AppDocument;
  moduleFiles: ReadonlyMap<string, string>;
}

interface DevServerEntry {
  handler(request: Request, overrides: DevOverrides): Promise<Response>;
}

interface DevOverrides {
  template: TemplateParts;
  headDefaults: TemplateHead;
  clientModules: ClientModules;
  errorHead: (error: unknown) => string;
}

const ServerEntryUrl = `/@id/${ServerEntryId.replace("\0", "__x00__")}`;
const StylesheetPattern = /\.(css|scss|sass|less|styl|stylus|pcss|postcss)$/;

export function createDevDocumentMiddleware(server: ViteDevServer, options: DevDocumentOptions): Connect.NextHandleFunction {
  const listener = createNodeListener((request) => renderDocument(server, options, request));
  return (req: IncomingMessage, res: ServerResponse, next) => {
    if (!isDocumentRequest(req)) {
      next();
      return;
    }
    listener(req, res);
  };
}

function isDocumentRequest(req: IncomingMessage): boolean {
  if (req.method !== "GET") return false;
  if (!(req.headers.accept ?? "").includes("text/html")) return false;
  const pathname = (req.url ?? "/").split("?")[0] ?? "/";
  if (pathname.startsWith("/@") || pathname.startsWith("/__reze/") || pathname.startsWith("/node_modules/")) return false;
  return !/\.[^/]+$/.test(pathname);
}

async function renderDocument(server: ViteDevServer, options: DevDocumentOptions, request: Request): Promise<Response> {
  const url = new URL(request.url);
  const errorHead = (error: unknown) => errorOverlayScript(server, error);
  let template: TemplateParts | undefined;
  try {
    const document = options.document();
    const html = await server.transformIndexHtml(url.pathname + url.search, document.html);
    const prepared = prepareTemplate(html, document.rootId);
    template = prepared.parts;
    const environment = server.environments[HtmlEnv];
    if (environment === undefined || !isRunnableDevEnvironment(environment)) {
      throw new Error(`[reze] ${HtmlEnv} environment is not runnable in dev`);
    }
    const entry = (await environment.runner.import(ServerEntryUrl)) as DevServerEntry;
    return await entry.handler(request, {
      template: prepared.parts,
      headDefaults: prepared.headDefaults,
      clientModules: devClientModules(server, options.moduleFiles),
      errorHead,
    });
  } catch (error) {
    server.config.logger.error(error instanceof Error ? (error.stack ?? error.message) : String(error));
    const html = buildErrorPage(template ?? BareDocument, errorHead(error));
    return new Response(html, { status: 500, headers: { "content-type": "text/html; charset=utf-8" } });
  }
}

const BareDocument: TemplateParts = {
  beforeHeadEnd: '<!doctype html><html><head><meta charset="utf-8">',
  headEndToRoot: "</head><body><div>",
  rootEndToBodyEnd: "</div></body>",
  bodyEndToEnd: "</html>",
};

function errorOverlayScript(server: ViteDevServer, error: unknown): string {
  const payload = error instanceof Error ? overlayPayload(error) : { message: String(error), stack: "" };
  const json = JSON.stringify(payload).replace(/</g, "\\u003c").replace(/\u2028/g, "\\u2028").replace(/\u2029/g, "\\u2029");
  const client = JSON.stringify(`${server.config.base}@vite/client`);
  return `<script type="module">import { ErrorOverlay } from ${client};\ndocument.body.appendChild(new ErrorOverlay(${json}));</script>`;
}

function overlayPayload(error: Error & { id?: unknown; frame?: unknown; plugin?: unknown; loc?: unknown }): Record<string, unknown> {
  return {
    message: error.message,
    stack: error.stack ?? "",
    id: error.id,
    frame: error.frame,
    plugin: error.plugin,
    loc: error.loc,
  };
}

function devClientModules(server: ViteDevServer, moduleFiles: ReadonlyMap<string, string>): ClientModules {
  const graph = server.environments[HtmlEnv]?.moduleGraph;
  return {
    url: (moduleId) => devFileUrl(moduleFileOf(moduleFiles, moduleId)),
    css: (moduleId) => {
      const file = moduleFiles.get(moduleId);
      if (graph === undefined || file === undefined) return [];
      return stylesheetsReachable(graph, file).map((href) => `${href}?direct`);
    },
    preload: () => [],
  };
}

function moduleFileOf(moduleFiles: ReadonlyMap<string, string>, moduleId: string): string {
  const file = moduleFiles.get(moduleId);
  if (file === undefined) throw new Error(`[reze] no dev module file for ${moduleId}`);
  return file;
}

function devFileUrl(file: string): string {
  return `/@fs${file.startsWith("/") ? "" : "/"}${file}`;
}

function stylesheetsReachable(graph: EnvironmentModuleGraph, file: string): string[] {
  const found = new Set<string>();
  const visited = new Set<EnvironmentModuleNode>();
  const visit = (node: EnvironmentModuleNode): void => {
    if (visited.has(node)) return;
    visited.add(node);
    const path = node.url.split("?")[0] ?? node.url;
    if (StylesheetPattern.test(path)) found.add(path);
    for (const imported of node.importedModules) visit(imported);
  };
  for (const node of graph.getModulesByFile(file) ?? []) visit(node);
  return [...found];
}
