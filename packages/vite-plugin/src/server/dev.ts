import { readFileSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";

import type { ClientModules } from "reze-js/internal/html";
import { createNodeListener } from "reze-js/node";
import { isRunnableDevEnvironment } from "vite";
import type { Connect, EnvironmentModuleGraph, EnvironmentModuleNode, ViteDevServer } from "vite";

import { HtmlEnv, ServerEntryId } from "./adapter";
import type { TemplateHead, TemplateParts } from "./template";
import { prepareTemplate } from "./template";

export interface DevDocumentOptions {
  templateFile: string;
  rootId: string;
  moduleFiles: ReadonlyMap<string, string>;
}

interface DevServerEntry {
  handler(request: Request, overrides: DevOverrides): Promise<Response>;
}

interface DevOverrides {
  template: TemplateParts;
  headDefaults: TemplateHead;
  clientModules: ClientModules;
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
  try {
    const html = await server.transformIndexHtml(url.pathname + url.search, readFileSync(options.templateFile, "utf8"));
    const prepared = prepareTemplate(html, options.rootId);
    const environment = server.environments[HtmlEnv];
    if (environment === undefined || !isRunnableDevEnvironment(environment)) {
      throw new Error(`[reze] ${HtmlEnv} environment is not runnable in dev`);
    }
    const entry = (await environment.runner.import(ServerEntryUrl)) as DevServerEntry;
    return await entry.handler(request, {
      template: prepared.parts,
      headDefaults: prepared.headDefaults,
      clientModules: devClientModules(server, options.moduleFiles),
    });
  } catch (error) {
    server.config.logger.error(error instanceof Error ? (error.stack ?? error.message) : String(error));
    return new Response("Internal Server Error", { status: 500, headers: { "content-type": "text/plain; charset=utf-8" } });
  }
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
