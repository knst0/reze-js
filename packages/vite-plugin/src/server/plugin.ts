import { existsSync } from "node:fs";
import { isAbsolute, resolve } from "node:path";

import { createRunnableDevEnvironment, isRunnableDevEnvironment, version as viteVersion } from "vite";
import type { EnvironmentOptions, Logger, Plugin, ViteDevServer } from "vite";

import { createModuleRegistry } from "../module-identity";
import type { ModuleRegistry } from "../module-identity";
import type { FileRoutesOptions } from "../routes";
import {
  ClientId,
  ClientRequest,
  DevFactsId,
  DocumentViewId,
  GeneratedIds,
  HtmlEnv,
  IslandsId,
  ServerEntryId,
  ServerFactsSpecifier,
  SsgRedirectId,
  SsgViewId,
  clientEntrySource,
  devFactsSource,
  documentViewSource,
  islandsSource,
  redirectModuleSource,
  serverEntrySource,
  viewSource,
} from "./adapter";
import { createAssetPlugins } from "./asset-plugin";
import type { ClientAssetInputs } from "./assets";
import type { CapturedState } from "./build";
import { renderBuiltDocument, runServerBuild } from "./build";
import { captureOutput } from "./capture";
import { createDevDocumentMiddleware } from "./dev";
import { analyzeDocument, renderedDocument } from "./document";
import type { AppDocument, ImportResolver } from "./document";
import { ClientOutDirDefault, assertDisjointOutDirs, assertSharedServerOptions, resolveSsgOptions, resolveSsrOptions } from "./options";
import type { ResolvedSsgOptions, ResolvedSsrOptions, SsgOptions, SsrOptions } from "./options";
import { validateTemplate } from "./template";

export interface ServerShared {
  enabled: boolean;
  root: string;
  isServe: boolean;
  registry: ModuleRegistry;
  moduleFiles: Map<string, string>;
}

export function createServerShared(): ServerShared {
  return {
    enabled: false,
    root: "",
    isServe: false,
    registry: createModuleRegistry(),
    moduleFiles: new Map(),
  };
}

export interface ServerOptions {
  ssg?: true | SsgOptions | undefined;
  ssr?: true | SsrOptions | undefined;
}

function viteMajor(): number {
  return Number(viteVersion.split(".")[0]);
}

function toPosixAbsolute(root: string, file: string): string {
  const absolute = (isAbsolute(file) ? file : resolve(root, file)).replace(/\\/g, "/");
  return /^[A-Za-z]:\//.test(absolute) ? `/${absolute}` : absolute;
}

const DocumentViewUrl = `/@id/${DocumentViewId.replace("\0", "__x00__")}`;

interface DocumentView {
  render(): Promise<string>;
}

function isServerEnvironment(environment: { name: string }): boolean {
  return environment.name === "client" || environment.name === HtmlEnv;
}

export function createServerPlugin(input: ServerOptions, shared: ServerShared, fileRoutes: FileRoutesOptions): Plugin[] {
  shared.enabled = true;
  const label = input.ssg === undefined ? "ssr" : "ssg";
  let root = "";
  let base = "/";
  let outDir = "";
  let publicDir = "";
  let isServe = false;
  let command = "";
  let ssgOptions: ResolvedSsgOptions | undefined;
  let ssrOptions: ResolvedSsrOptions | undefined;
  let primary: ResolvedSsgOptions | ResolvedSsrOptions | undefined;
  let documentFile = "";
  let htmlInput = "";
  let devServer: ViteDevServer | undefined;
  let logger: Logger | undefined;
  let appDocument: Promise<AppDocument> | undefined;
  let documentInputs: ReadonlySet<string> | undefined;
  let documentViewRef: string | undefined;
  let documentViewFile: string | undefined;
  const currentDocument = (): Promise<AppDocument> => {
    if (appDocument !== undefined) return appDocument;
    if (!isServe) throw new Error("[reze] the document is read before the HTML build analyzed it");
    const loading = loadDevDocument().then((document) => {
      documentInputs = document.files === undefined ? undefined : new Set([documentFile, ...document.files]);
      return document;
    });
    appDocument = loading;
    loading.catch(() => {
      if (appDocument === loading) appDocument = undefined;
    });
    return loading;
  };

  async function loadDevDocument(): Promise<AppDocument> {
    const environment = devServer?.environments[HtmlEnv];
    if (environment === undefined || !isRunnableDevEnvironment(environment)) {
      throw new Error(`[reze] ${HtmlEnv} environment is not runnable in dev`);
    }
    const resolveImport: ImportResolver = async (specifier, importer) =>
      (await environment.pluginContainer.resolveId(specifier, importer))?.id;
    const analysis = await analyzeDocument(documentFile, resolveImport);
    if (analysis.status === "static") return analysis.document;
    logger?.info(analysis.reasons);
    const view = (await environment.runner.import(DocumentViewUrl)) as DocumentView;
    return renderedDocument(await view.render(), primary!.document);
  }
  let redirectRef: string | undefined;
  const state: CapturedState = { outputs: [], templateHtml: "", templateFile: "", redirectFile: "", htmlChunks: [] };
  const assets: ClientAssetInputs = { files: new Map(), inlined: new Map(), publicFiles: new Map() };

  const virtuals: Plugin = {
    name: "reze-server-virtuals",
    enforce: "pre",
    applyToEnvironment: isServerEnvironment,
    resolveId(id, importer) {
      if (id === ClientRequest) return ClientId;
      if (!isServe && id === htmlInput && this.environment.name === "client") return id;
      if (isServe && id === ServerFactsSpecifier && importer === ServerEntryId) return DevFactsId;
      if (GeneratedIds.includes(id) || id === SsgViewId || id === DocumentViewId) return id;
      return undefined;
    },
    async load(id) {
      if (primary === undefined) return undefined;
      if (!isServe && id === htmlInput && this.environment.name === "client") return (await currentDocument()).html;
      if (id === ClientId) return clientEntrySource((await currentDocument()).rootId);
      if (id === IslandsId) return { code: islandsSource(islandEntries()), moduleSideEffects: "no-treeshake" };
      if (id === ServerEntryId) {
        return this.environment.name === HtmlEnv ? serverEntrySource() : undefined;
      }
      if (id === SsgViewId) return viewSource();
      if (id === DocumentViewId) return documentViewSource(documentFile, primary.timeoutMs);
      if (id === SsgRedirectId) return redirectModuleSource();
      if (id === DevFactsId) {
        return devFactsSource({ base, rootId: (await currentDocument()).rootId, timeoutMs: (ssrOptions ?? primary).timeoutMs });
      }
      return undefined;
    },
  };

  function islandEntries(): { moduleId: string; file: string }[] {
    if (isServe) return [];
    return [...shared.moduleFiles].map(([moduleId, file]) => ({ moduleId, file }));
  }

  const coordinator: Plugin = {
    name: "reze-server",
    enforce: "pre",
    applyToEnvironment: isServerEnvironment,
    config(config, env) {
      command = env.command;
      if (fileRoutes.history === "hash") {
        throw new Error(`[reze] ${label} does not support hash routing`);
      }
      if (config.build?.lib !== undefined && config.build.lib !== false) {
        throw new Error(`[reze] ${label} does not support library mode`);
      }
      if (input.ssr !== undefined && config.build?.outDir === undefined) {
        config.build = { ...config.build, outDir: ClientOutDirDefault };
      }
      const environments = config.environments ?? {};
      const htmlEnv: EnvironmentOptions = environments[HtmlEnv] ?? {};
      const htmlBuild = htmlEnv.build ?? {};
      const adapterInput = { "reze-server-html": ServerEntryId };
      const external = [ServerFactsSpecifier];
      const output = { paths: { [ServerFactsSpecifier]: `./${ServerFactsSpecifier}` } };
      if (viteMajor() >= 8) {
        htmlEnv.build = {
          ...htmlBuild,
          write: false,
          rolldownOptions: { ...htmlBuild.rolldownOptions, input: adapterInput, external, output },
        };
      } else {
        htmlEnv.build = {
          ...htmlBuild,
          write: false,
          rollupOptions: { ...htmlBuild.rollupOptions, input: adapterInput, external, output },
        };
      }
      if (command === "serve") {
        htmlEnv.dev = {
          ...htmlEnv.dev,
          createEnvironment: (name, environmentConfig) => createRunnableDevEnvironment(name, environmentConfig),
        };
      }
      htmlEnv.consumer = "server";
      config.environments = { ...environments, [HtmlEnv]: htmlEnv };
      config.builder = {
        ...config.builder,
        sharedConfigBuild: true,
        sharedPlugins: true,
        async buildApp(builder) {
          await builder.build(builder.environments[HtmlEnv]!);
          if (primary !== undefined && documentViewFile !== undefined) {
            const rendered = await renderBuiltDocument(root, state.htmlChunks, documentViewFile, primary.timeoutMs);
            appDocument = Promise.resolve(renderedDocument(rendered, primary.document));
          }
          await builder.build(builder.environments.client!);
          if (primary === undefined || state.outputs.length === 0 || state.htmlChunks.length === 0 || state.templateHtml === "") {
            throw new Error("[reze] SSG build did not produce both the executable HTML graph and client template");
          }
          await runServerBuild({
            root,
            base,
            clientOutDir: outDir,
            publicDir,
            captured: { ...state, htmlChunks: state.htmlChunks.filter((chunk) => chunk.fileName !== documentViewFile) },
            assets,
            moduleFiles: shared.moduleFiles,
            rootId: (await currentDocument()).rootId,
            ssg: ssgOptions,
            ssr: ssrOptions,
          });
        },
      };
    },
    configEnvironment(name, envConfig) {
      if (name !== HtmlEnv) return;
      envConfig.resolve = { ...envConfig.resolve, noExternal: true };
    },
    configResolved(config) {
      root = config.root;
      logger = config.logger;
      base = config.base;
      outDir = resolve(root, config.build.outDir);
      publicDir = config.publicDir;
      isServe = config.command === "serve";
      shared.root = root;
      shared.isServe = isServe;
      ssgOptions = input.ssg === undefined ? undefined : resolveSsgOptions(input.ssg);
      const resolvedSsr = input.ssr === undefined ? undefined : resolveSsrOptions(input.ssr);
      ssrOptions = resolvedSsr === undefined ? undefined : { ...resolvedSsr, outDir: resolve(root, resolvedSsr.outDir) };
      if (ssrOptions !== undefined) assertDisjointOutDirs(outDir, ssrOptions.outDir);
      if (ssgOptions !== undefined && ssrOptions !== undefined) assertSharedServerOptions(ssgOptions, ssrOptions);
      primary = ssgOptions ?? ssrOptions;
      if (primary === undefined) throw new Error("[reze] server plugin needs ssg or ssr options");
      if (!isServe) {
        if (config.build.watch !== null && config.build.watch !== undefined) {
          throw new Error(`[reze] ${label} does not support build.watch`);
        }
        if (config.build.ssr === true || typeof config.build.ssr === "string") {
          throw new Error(`[reze] ${label} does not support an explicit ssr build; it owns the HTML environment itself`);
        }
        checkBundlerInput(config.build.rollupOptions, "rollupOptions");
        checkBundlerInput(config.build.rolldownOptions, "rolldownOptions");
      }
      htmlInput = toPosixAbsolute(root, "index.html");
      if (existsSync(htmlInput)) {
        throw new Error(
          `[reze] ${label} renders every page from ${JSON.stringify(primary.document)} and never reads index.html; move its <head> tags into that document and delete index.html`,
        );
      }
      documentFile = toPosixAbsolute(root, primary.document);
      appDocument = undefined;
      documentViewRef = undefined;
      documentViewFile = undefined;
    },
    configureServer(server) {
      devServer = server;
      if (primary === undefined) return;
      server.middlewares.use(
        createDevDocumentMiddleware(server, {
          document: currentDocument,
          moduleFiles: shared.moduleFiles,
          context: ssrOptions?.devContext,
        }),
      );
    },
    async buildStart() {
      if (isServe) return;
      if (this.environment.name === "client") {
        redirectRef = this.emitFile({ type: "chunk", id: SsgRedirectId, name: "reze-ssg-redirect" });
      }
      if (this.environment.name === HtmlEnv) {
        const analysis = await analyzeDocument(documentFile, async (specifier, importer) => (await this.resolve(specifier, importer))?.id);
        if (analysis.status === "static") {
          appDocument = Promise.resolve(analysis.document);
        } else {
          logger?.info(analysis.reasons);
          documentViewRef = this.emitFile({ type: "chunk", id: DocumentViewId, name: "reze-document" });
        }
      }
    },
    transformIndexHtml: {
      order: "pre",
      async handler(html) {
        if (primary === undefined) return html;
        validateTemplate(html, primary.document, (await currentDocument()).rootId, ClientRequest);
        return html;
      },
    },
    generateBundle: {
      order: "post",
      handler(_bundleOptions, bundle) {
        const envName = this.environment.name;
        if (envName !== "client" && envName !== HtmlEnv) return;
        for (const value of Object.values(bundle)) {
          const found = captureOutput(value);
          if (found === undefined) continue;
          if (envName === HtmlEnv) {
            if (found.output.type === "chunk") state.htmlChunks.push(found.output);
          } else {
            state.outputs.push(found.output);
            if (found.templateHtml !== undefined) {
              state.templateHtml = found.templateHtml;
              state.templateFile = found.output.fileName;
              delete bundle[found.output.fileName];
            }
          }
        }
        if (envName === "client" && redirectRef !== undefined) {
          state.redirectFile = this.getFileName(redirectRef);
        }
        if (envName === HtmlEnv && documentViewRef !== undefined) {
          documentViewFile = this.getFileName(documentViewRef);
          const view = bundle[documentViewFile];
          const documentOnly = new Set(view?.type === "chunk" ? view.moduleIds : []);
          for (const [moduleId, file] of shared.moduleFiles) {
            if (documentOnly.has(file)) shared.moduleFiles.delete(moduleId);
          }
        }
      },
    },
    hotUpdate({ file, server }) {
      if (appDocument !== undefined && (documentInputs === undefined || documentInputs.has(file))) {
        appDocument = undefined;
        const graph = this.environment.moduleGraph;
        for (const id of [ClientId, DevFactsId]) {
          const node = graph.getModuleById(id);
          if (node !== undefined) graph.invalidateModule(node);
        }
      }
      if (!file.startsWith(`${root}/`) || file.includes("/node_modules/")) return;
      server.ws.send({ type: "full-reload" });
      return [];
    },
  };
  return [virtuals, coordinator, ...createAssetPlugins(HtmlEnv, assets)];
}

function checkBundlerInput(value: unknown, key: string): void {
  if (typeof value !== "object" || value === null) return;
  if ("input" in value && value.input !== undefined) {
    throw new Error(`[reze] ssg owns the bundler input; remove build.${key}.input from the config`);
  }
  if (!("output" in value)) return;
  const outputs = Array.isArray(value.output) ? value.output : [value.output];
  for (const output of outputs) {
    if (typeof output === "object" && output !== null && "inlineDynamicImports" in output && output.inlineDynamicImports === true) {
      throw new Error(`[reze] ssg needs preserved dynamic chunks; remove build.${key}.output.inlineDynamicImports`);
    }
  }
}
