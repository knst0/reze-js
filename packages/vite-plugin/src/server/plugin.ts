import { existsSync, readFileSync } from "node:fs";
import { isAbsolute, join, resolve } from "node:path";

import { createRunnableDevEnvironment, version as viteVersion } from "vite";
import type { EnvironmentOptions, Plugin } from "vite";

import { createModuleRegistry } from "../module-identity";
import type { ModuleRegistry } from "../module-identity";
import type { FileRoutesOptions } from "../routes";
import {
  ClientId,
  ClientRequest,
  DevFactsId,
  GeneratedIds,
  HtmlEnv,
  IslandsId,
  ServerEntryId,
  ServerFactsSpecifier,
  SsgRedirectId,
  SsgViewId,
  clientEntrySource,
  devFactsSource,
  islandsSource,
  redirectModuleSource,
  serverEntrySource,
  viewSource,
} from "./adapter";
import { createAssetPlugins } from "./asset-plugin";
import type { ClientAssetInputs } from "./assets";
import type { CapturedState } from "./build";
import { runServerBuild } from "./build";
import { captureOutput } from "./capture";
import { createDevDocumentMiddleware } from "./dev";
import { resolveAppMode } from "./export-graph";
import type { AppMode } from "./export-graph";
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
  ssg?: SsgOptions | undefined;
  ssr?: SsrOptions | undefined;
}

function viteMajor(): number {
  return Number(viteVersion.split(".")[0]);
}

function toPosixAbsolute(root: string, entry: string): string {
  const absolute = (isAbsolute(entry) ? entry : resolve(root, entry)).replace(/\\/g, "/");
  return /^[A-Za-z]:\//.test(absolute) ? `/${absolute}` : absolute;
}

function isServerEnvironment(environment: { name: string }): boolean {
  return environment.name === "client" || environment.name === HtmlEnv;
}

export function createServerPlugin(
  input: ServerOptions,
  shared: ServerShared,
  fileRoutes: false | FileRoutesOptions | undefined,
): Plugin[] {
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
  let mode: AppMode | undefined;
  let entryAbs = "";
  let redirectRef: string | undefined;
  const state: CapturedState = { outputs: [], templateHtml: "", templateFile: "", redirectFile: "", htmlChunks: [] };
  const assets: ClientAssetInputs = { files: new Map(), inlined: new Map(), publicFiles: new Map() };

  const virtuals: Plugin = {
    name: "reze-server-virtuals",
    enforce: "pre",
    applyToEnvironment: isServerEnvironment,
    resolveId(id, importer) {
      if (id === ClientRequest) return ClientId;
      if (isServe && id === ServerFactsSpecifier && importer === ServerEntryId) return DevFactsId;
      if (GeneratedIds.includes(id) || id === SsgViewId) return id;
      return undefined;
    },
    load(id) {
      if (primary === undefined || mode === undefined) return undefined;
      if (id === ClientId) return clientEntrySource({ router: mode.kind === "router", rootId: primary.rootId });
      if (id === IslandsId) return { code: islandsSource(islandEntries()), moduleSideEffects: "no-treeshake" };
      if (id === ServerEntryId) {
        return this.environment.name === HtmlEnv ? serverEntrySource({ mode, entrySpecifier: entryAbs }) : undefined;
      }
      if (id === SsgViewId) return viewSource(mode, entryAbs);
      if (id === SsgRedirectId) return redirectModuleSource();
      if (id === DevFactsId) {
        return devFactsSource({ base, rootId: primary.rootId, timeoutMs: (ssrOptions ?? primary).timeoutMs });
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
      if (fileRoutes !== false && fileRoutes !== undefined && fileRoutes.history === "hash") {
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
          await builder.build(builder.environments.client!);
          if (
            mode === undefined ||
            primary === undefined ||
            state.outputs.length === 0 ||
            state.htmlChunks.length === 0 ||
            state.templateHtml === ""
          ) {
            throw new Error("[reze] SSG build did not produce both the executable HTML graph and client template");
          }
          await runServerBuild({
            root,
            base,
            clientOutDir: outDir,
            publicDir,
            mode,
            captured: state,
            assets,
            moduleFiles: shared.moduleFiles,
            ssg: ssgOptions,
            ssr: ssrOptions,
          });
        },
      };
    },
    configEnvironment(name, envConfig) {
      if (name !== HtmlEnv) return;
      const resolveOptions = envConfig.resolve ?? {};
      const conditions = resolveOptions.conditions ?? [];
      if (!conditions.includes("reze-html")) conditions.push("reze-html");
      resolveOptions.noExternal = true;
      resolveOptions.conditions = conditions;
      envConfig.resolve = resolveOptions;
    },
    configResolved(config) {
      root = config.root;
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
      entryAbs = toPosixAbsolute(root, primary.entry);
      if (!existsSync(entryAbs)) {
        throw new Error(`[reze] ${label}.entry not found: ${JSON.stringify(primary.entry)}`);
      }
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
      const templateFile = resolve(root, primary.template);
      if (!existsSync(templateFile)) {
        throw new Error(`[reze] ${label} template not found: ${JSON.stringify(primary.template)}`);
      }
      validateTemplate(readFileSync(templateFile, "utf8"), primary.template, primary.rootId, ClientRequest);
      mode = resolveAppMode(entryAbs, {
        readFile: (id) => (existsSync(id) ? readFileSync(id, "utf8") : undefined),
        resolveSpec: (spec, importer) => resolveImport(spec, importer, root),
        virtualExports: (id) => (id === "virtual:reze-routes" || id === "\0virtual:reze-routes" ? ["routes", "paths"] : undefined),
      });
      if (mode.kind === "standalone" && ssgOptions !== undefined && Object.keys(ssgOptions.paths).length > 0) {
        throw new Error("[reze] ssg.paths needs a router app; a standalone entry renders only /");
      }
    },
    configureServer(server) {
      if (primary === undefined) return;
      server.middlewares.use(
        createDevDocumentMiddleware(server, {
          templateFile: resolve(root, primary.template),
          rootId: primary.rootId,
          moduleFiles: shared.moduleFiles,
        }),
      );
    },
    buildStart() {
      if (this.environment.name === "client" && !isServe) {
        redirectRef = this.emitFile({ type: "chunk", id: SsgRedirectId, name: "reze-ssg-redirect" });
      }
    },
    transformIndexHtml: {
      order: "pre",
      handler(html) {
        if (primary === undefined) return html;
        validateTemplate(html, primary.template, primary.rootId, ClientRequest);
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
      },
    },
    hotUpdate({ file, server }) {
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

function resolveImport(spec: string, importer: string, root: string): string | undefined {
  if (spec === "virtual:reze-routes") return "virtual:reze-routes";
  if (!spec.startsWith("./") && !spec.startsWith("../") && !spec.startsWith("/")) return undefined;
  const basePath = spec.startsWith("/") ? join(root, spec.slice(1)) : join(importer.slice(0, importer.lastIndexOf("/")), spec);
  for (const candidate of [
    basePath,
    `${basePath}.ts`,
    `${basePath}.tsx`,
    `${basePath}.js`,
    `${basePath}.jsx`,
    `${basePath}.mts`,
    `${basePath}.cts`,
    `${basePath}.mjs`,
    `${basePath}.cjs`,
    `${basePath}/index.ts`,
    `${basePath}/index.tsx`,
    `${basePath}/index.js`,
  ]) {
    if (existsSync(candidate)) return candidate.replace(/\\/g, "/");
  }
  return undefined;
}
