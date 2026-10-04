import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { isAbsolute, join, resolve } from "node:path";
import { version as viteVersion } from "vite";

import type { EnvironmentOptions, Plugin } from "vite";

import { canonicalModuleId, createModuleRegistry } from "../module-identity";
import type { ModuleRegistry } from "../module-identity";
import type { FileRoutesOptions } from "../routes";
import { buildClientRegistry, decideAssetImport } from "./assets";
import type { BundleOutput, ClientRegistry } from "./assets";
import { SsgClientId, SsgClientRequest, SsgHtmlAdapterId, SsgRedirectId, SsgViewId, clientBootSource, devBootSource, htmlAdapterSource, redirectModuleSource, viewSource, workerEntrySource } from "./adapter";
import { resolveAppMode } from "./export-graph";
import type { AppMode } from "./export-graph";
import { resolvePathsCallbacks, resolveSsgOptions } from "./options";
import type { ResolvedSsgOptions, SsgOptions } from "./options";
import { bootstrapScriptSrc, buildPage, buildRedirectPage, countRootIds, readHeadDefaults, validateTemplate } from "./template";
import { canonicalPageUrl, joinBase, normalizePageUrl, outputFileFor, pageDepth, planOutputs } from "./urls";
import { createTempDir, materializeBundle, removeTempDir, runWorker } from "./workers";

export const HtmlEnv = "reze_html";

export interface SsgShared {
  enabled: boolean;
  root: string;
  isServe: boolean;
  include: (id: string) => boolean;
  limit: number;
  registry: ModuleRegistry;
  moduleFiles: Map<string, string>;
}

export function createSsgShared(): SsgShared {
  return {
    enabled: false,
    root: "",
    isServe: false,
    include: (id) => /\.(?:png|jpe?g|gif|svg|webp|avif|woff2?|ttf|otf|mp4|webm|mp3|wav|ogg)$/i.test(id),
    limit: 4096,
    registry: createModuleRegistry(),
    moduleFiles: new Map(),
  };
}

interface DiscoverResult {
  mode: "router" | "standalone";
  descriptors: unknown;
  urls: { url: string; leafId: string; params: Record<string, string> }[];
}
interface RenderPortal {
  placement: string;
  token: string;
  html: string;
}

interface RenderResult {
  status: "render" | "redirect";
  html?: string | undefined;
  portals?: RenderPortal[] | undefined;
  payload?: string | undefined;
  modules?: readonly string[];
  metadata?: { title?: string; description?: string; canonical?: string; robots?: string } | undefined;
  to?: string | undefined;
  replace?: boolean | undefined;
}
interface CapturedState {
  outputs: BundleOutput[];
  templateHtml: string;
  templateFile: string;
  redirectFile: string;
  htmlChunks: (BundleOutput & { code?: string | undefined })[];
}

function viteMajor(): number {
  return Number(viteVersion.split(".")[0]);
}

function toPosixAbsolute(root: string, entry: string): string {
  const absolute = (isAbsolute(entry) ? entry : resolve(root, entry)).replace(/\\/g, "/");
  return /^[A-Za-z]:\//.test(absolute) ? `/${absolute}` : absolute;
}

export function createSsgPlugin(input: SsgOptions, shared: SsgShared, fileRoutes: false | FileRoutesOptions | undefined): Plugin[] {
  shared.enabled = true;
  let root = "";
  let base = "/";
  let outDir = "";
  let publicDir = "";
  let isServe = false;
  let command = "";
  let options: ResolvedSsgOptions | undefined;
  let mode: AppMode | undefined;
  let entryAbs = "";
  const state: CapturedState = { outputs: [], templateHtml: "", templateFile: "", redirectFile: "", htmlChunks: [] };
  let redirectRef: string | undefined;

  const virtuals: Plugin = {
    name: "reze-ssg-virtuals",
    enforce: "pre",
    applyToEnvironment: (environment) => environment.name === "client" || environment.name === HtmlEnv,
    resolveId(id) {
      if (id === SsgClientRequest) return SsgClientId;
      if (id === SsgViewId || id === SsgHtmlAdapterId || id === SsgRedirectId) return id;
      return undefined;
    },
    load(id) {
      if (options === undefined || mode === undefined) return undefined;
      if (id === SsgClientId) return isServe ? devBootSource(options.rootId) : clientBootSource({ mode, rootId: options.rootId });
      if (id === SsgViewId) return viewSource(mode, entryAbs);
      if (id === SsgHtmlAdapterId) {
        if (this.environment.name !== HtmlEnv) return undefined;
        return htmlAdapterSource({ mode, entrySpecifier: entryAbs });
      }
      if (id === SsgRedirectId) return redirectModuleSource();
      return undefined;
    },
  };

  const coordinator: Plugin = {
    name: "reze-ssg",
    enforce: "pre",
    applyToEnvironment: (environment) => environment.name === "client" || environment.name === HtmlEnv,
    config(config, env) {
      command = env.command;
      if (fileRoutes !== false && fileRoutes !== undefined && fileRoutes.history === "hash") {
        throw new Error("[reze] ssg does not support hash routing");
      }
      if (config.build?.lib !== undefined && config.build.lib !== false) {
        throw new Error("[reze] ssg does not support library mode");
      }
      const environments = config.environments ?? {};
      const htmlEnv: EnvironmentOptions = environments[HtmlEnv] ?? {};
      const htmlBuild = htmlEnv.build ?? {};
      const adapterInput = { "reze-ssg-html": SsgHtmlAdapterId };
      if (viteMajor() >= 8) {
        htmlEnv.build = { ...htmlBuild, write: false, rolldownOptions: { ...htmlBuild.rolldownOptions, input: adapterInput } };
      } else {
        htmlEnv.build = { ...htmlBuild, write: false, rollupOptions: { ...htmlBuild.rollupOptions, input: adapterInput } };
      }
      htmlEnv.consumer = "server";
      config.environments = { ...environments, [HtmlEnv]: htmlEnv };
      config.builder = { ...config.builder, sharedConfigBuild: true, sharedPlugins: true };
    },
    configEnvironment(name, envConfig) {
      const resolve = envConfig.resolve ?? {};
      const conditions = resolve.conditions ?? [];
      if (name === HtmlEnv) {
        if (!conditions.includes("reze-html")) conditions.push("reze-html");
        resolve.noExternal = true;
      } else if (name === "client" && command === "build") {
        if (!conditions.includes("reze-hydrate")) conditions.push("reze-hydrate");
      }
      resolve.conditions = conditions;
      envConfig.resolve = resolve;
    },
    configResolved(config) {
      root = config.root;
      base = config.base;
      outDir = resolve(root, config.build.outDir);
      publicDir = resolve(root, config.publicDir);
      isServe = config.command === "serve";
      shared.root = root;
      shared.isServe = isServe;
      const rawLimit: unknown = config.build.assetsInlineLimit;
      shared.limit = rawLimit === false ? -1 : (typeof rawLimit === "number" ? rawLimit : 4096);
      const assetsInclude: unknown = "assetsInclude" in config ? config.assetsInclude : undefined;
      shared.include = includePredicate(assetsInclude);
      options = resolveSsgOptions(input, root);
      entryAbs = toPosixAbsolute(root, options.entry);
      if (!existsSync(entryAbs)) {
        throw new Error(`[reze] ssg.entry not found: ${JSON.stringify(options.entry)}`);
      }
      if (!isServe) {
        if (config.build.watch !== null && config.build.watch !== undefined) {
          throw new Error("[reze] ssg does not support build.watch");
        }
        if (config.build.ssr === true || typeof config.build.ssr === "string") {
          throw new Error("[reze] ssg does not support an explicit ssr build; it owns the HTML environment itself");
        }
        checkBundlerInput(config.build.rollupOptions, "rollupOptions");
        checkBundlerInput(config.build.rolldownOptions, "rolldownOptions");
      }
      const templateFile = resolve(root, options.template);
      if (!existsSync(templateFile)) {
        throw new Error(`[reze] ssg template not found: ${JSON.stringify(options.template)}`);
      }
      validateTemplate(readFileSync(templateFile, "utf8"), options.template, options.rootId, SsgClientRequest);
      mode = resolveAppMode(entryAbs, {
        readFile: (id) => (existsSync(id) ? readFileSync(id, "utf8") : undefined),
        resolveSpec: (spec, importer) => resolveImport(spec, importer, root),
        virtualExports: (id) => (id === "virtual:reze-routes" || id === "\0virtual:reze-routes" ? ["routes", "paths"] : undefined),
      });
      if (mode.kind === "standalone" && Object.keys(options.paths).length > 0) {
        throw new Error("[reze] ssg.paths needs a router app; a standalone entry renders only /");
      }
    },
    async buildApp(builder) {
      await builder.build(builder.environments[HtmlEnv]!);
      await builder.build(builder.environments.client!);
      if (options === undefined || mode === undefined || state.outputs.length === 0 || state.htmlChunks.length === 0 || state.templateHtml === "") {
        throw new Error("[reze] SSG build did not produce both the executable HTML graph and client template");
      }
      await runSsgBuild({ root, base, outDir, publicDir, options, mode, captured: state, moduleFiles: shared.moduleFiles, modules: shared.registry.ids() });
    },
    buildStart() {
      if (this.environment.name === "client" && !isServe) {
        redirectRef = this.emitFile({ type: "chunk", id: SsgRedirectId, name: "reze-ssg-redirect" });
      }
    },
    transformIndexHtml: {
      order: "pre",
      handler(html) {
        if (options === undefined) return html;
        validateTemplate(html, options.template, options.rootId, SsgClientRequest);
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
  };
  return [virtuals, coordinator];
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

function readStrings(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((entry): entry is string => typeof entry === "string");
}

function readStringSet(value: unknown): Set<string> | readonly string[] | undefined {
  if (value instanceof Set) {
    const out: string[] = [];
    for (const entry of value) {
      if (typeof entry === "string") out.push(entry);
    }
    return out;
  }
  if (Array.isArray(value)) return readStrings(value);
  return undefined;
}

function decodeSource(source: unknown): string {
  if (typeof source === "string") return source;
  if (source instanceof Uint8Array) return Buffer.from(source).toString("utf8");
  throw new Error("[reze] built template index.html has no readable source");
}

function captureOutput(value: unknown): { output: BundleOutput; templateHtml?: string } | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  if (!("type" in value) || !("fileName" in value)) return undefined;
  if (typeof value.fileName !== "string") return undefined;
  if (value.type === "chunk") {
    if (!("code" in value) || typeof value.code !== "string") return undefined;
    const metadata = "viteMetadata" in value && typeof value.viteMetadata === "object" && value.viteMetadata !== null
      ? value.viteMetadata
      : undefined;
    const importedCss = metadata !== undefined && "importedCss" in metadata ? readStringSet(metadata.importedCss) : undefined;
    const importedAssets = metadata !== undefined && "importedAssets" in metadata ? readStringSet(metadata.importedAssets) : undefined;
    return {
      output: {
        type: "chunk",
        fileName: value.fileName,
        code: value.code,
        facadeModuleId: "facadeModuleId" in value && typeof value.facadeModuleId === "string" ? value.facadeModuleId : undefined,
        moduleIds: "moduleIds" in value ? readStrings(value.moduleIds) : [],
        imports: "imports" in value ? readStrings(value.imports) : [],
        dynamicImports: "dynamicImports" in value ? readStrings(value.dynamicImports) : [],
        isEntry: "isEntry" in value && value.isEntry === true,
        viteMetadata: { importedAssets, importedCss },
      },
    };
  }
  if (value.type === "asset") {
    const output: BundleOutput = {
      type: "asset",
      fileName: value.fileName,
      originalFileName: "originalFileName" in value && typeof value.originalFileName === "string" ? value.originalFileName : undefined,
      originalFileNames: "originalFileNames" in value ? readStrings(value.originalFileNames) : undefined,
    };
    if (value.fileName !== "index.html") return { output };
    if (!("source" in value)) throw new Error("[reze] built template index.html has no readable source");
    return { output, templateHtml: decodeSource(value.source) };
  }
  return undefined;
}

export function includePredicate(assetsInclude: unknown): (id: string) => boolean {
  if (assetsInclude === undefined) {
    return (id) => /\.(?:png|jpe?g|gif|svg|webp|avif|woff2?|ttf|otf|mp4|webm|mp3|wav|ogg)$/i.test(id);
  }
  const list = Array.isArray(assetsInclude) ? assetsInclude : [assetsInclude];
  const tests = list.map((entry) => {
    if (typeof entry === "string") return (id: string) => id.includes(entry);
    if (entry instanceof RegExp) return (id: string) => entry.test(id);
    return (_id: string) => false;
  });
  return (id) => tests.some((test) => test(id));
}

function resolveImport(spec: string, importer: string, root: string): string | undefined {
  if (spec === "virtual:reze-routes") return "virtual:reze-routes";
  if (!spec.startsWith("./") && !spec.startsWith("../") && !spec.startsWith("/")) return undefined;
  const basePath = spec.startsWith("/") ? join(root, spec.slice(1)) : join(importer.slice(0, importer.lastIndexOf("/")), spec);
  for (const candidate of [basePath, `${basePath}.ts`, `${basePath}.tsx`, `${basePath}.js`, `${basePath}.jsx`, `${basePath}.mts`, `${basePath}.cts`, `${basePath}.mjs`, `${basePath}.cjs`, `${basePath}/index.ts`, `${basePath}/index.tsx`, `${basePath}/index.js`]) {
    if (existsSync(candidate)) return candidate.replace(/\\/g, "/");
  }
  return undefined;
}

// Rewrites emitted-file asset references in the HTML environment to canonical
// registry lookups. Runs on compiled output so compiler diagnostics keep
// their source positions. Inline and raw assets keep Vite's deterministic
// output in both environments.
export function transformHtmlAsset(code: string, id: string, root: string, include: (file: string) => boolean, limit: number): string | undefined {
  const file = id.replace(/[?#].*$/, "");
  const dir = file.slice(0, file.lastIndexOf("/"));
  let helper = false;
  const rewritten = code
    .replace(/import\s+([A-Za-z_$][\w$]*)\s+from\s*("|')([^"']+)\2\s*;?/g, (match, local: string, _quote: string, spec: string) => {
      if (!spec.startsWith("./") && !spec.startsWith("../") && !spec.startsWith("/")) return match;
      const queryIndex = spec.indexOf("?");
      const specPath = queryIndex < 0 ? spec : spec.slice(0, queryIndex);
      const query = queryIndex < 0 ? "" : spec.slice(queryIndex + 1);
      const assetFile = (specPath.startsWith("/") ? join(root, specPath.slice(1)) : join(dir, specPath)).replace(/\\/g, "/");
      if (decideAssetImport({ file: assetFile, query, assetsInclude: include, inlineLimit: limit }) !== "lookup") return match;
      helper = true;
      return `const ${local} = htmlAsset(${JSON.stringify(canonicalModuleId(assetFile, root))});`;
    })
    .replace(/new\s+URL\(\s*("|')([^"']+)\1\s*,\s*import\.meta\.url\s*\)/g, (match, _quote: string, spec: string) => {
      if (!spec.startsWith("./") && !spec.startsWith("../")) return match;
      const assetFile = join(dir, spec).replace(/\\/g, "/");
      if (!existsSync(assetFile) || !include(assetFile)) return match;
      helper = true;
      return `htmlAsset(${JSON.stringify(canonicalModuleId(assetFile, root))})`;
    });
  if (!helper || rewritten === code) return undefined;
  if (!rewritten.includes('from "reze-js/internal/html"')) {
    return `import { htmlAsset } from "reze-js/internal/html";\n${rewritten}`;
  }
  return rewritten;
}

interface SsgBuildInput {
  root: string;
  base: string;
  outDir: string;
  publicDir: string;
  options: ResolvedSsgOptions;
  mode: AppMode;
  captured: CapturedState;
  moduleFiles: ReadonlyMap<string, string>;
  modules: readonly string[];
}
function assertDiscovery(discovery: DiscoverResult): void {
  if (discovery.mode !== "router" && discovery.mode !== "standalone") {
    throw new Error("[reze] router discovery returned an unknown app mode");
  }
  if (!Array.isArray(discovery.urls)) throw new Error("[reze] router discovery returned no URL list");
  for (const entry of discovery.urls) {
    if (typeof entry !== "object" || entry === null || typeof entry.url !== "string" || typeof entry.leafId !== "string") {
      throw new Error("[reze] router discovery returned a malformed URL entry");
    }
  }
}

function assertPage(url: string, page: RenderResult): void {
  if (page.status === "redirect") {
    if (page.to !== undefined && typeof page.to !== "string") {
      throw new Error(`[reze] SSG render of ${JSON.stringify(url)} returned a malformed redirect`);
    }
    return;
  }
  if (page.status !== "render" || typeof page.html !== "string" || typeof page.payload !== "string" || !Array.isArray(page.portals)
    || !Array.isArray(page.modules) || page.modules.some(module => typeof module !== "string")) {
    throw new Error(`[reze] SSG render of ${JSON.stringify(url)} returned a malformed page`);
  }
  for (const portal of page.portals) {
    if (typeof portal !== "object" || portal === null || typeof portal.token !== "string" || typeof portal.html !== "string" || typeof portal.placement !== "string") {
      throw new Error(`[reze] SSG render of ${JSON.stringify(url)} returned a malformed portal`);
    }
  }
}

async function runSsgBuild(input: SsgBuildInput): Promise<void> {
  const { options, mode, captured } = input;
  const registry = buildClientRegistry(captured.outputs, input.root);
  const buildId = registry.entryChunk(SsgClientId).fileName;
  bootstrapScriptSrc(captured.templateHtml, buildId);
  if (countRootIds(captured.templateHtml, options.rootId) !== 1) {
    throw new Error(`[reze] built template must contain exactly one element with id ${JSON.stringify(options.rootId)}`);
  }
  const headDefaults = readHeadDefaults(captured.templateHtml);
  const paths = await resolvePathsCallbacks(options.paths);
  const tempDir = createTempDir(input.root);
  try {
    const { files } = materializeBundle(captured.htmlChunks, tempDir);
    const adapterChunk = [...files.keys()].find((file) => file.includes("reze-ssg-html"));
    if (adapterChunk === undefined) throw new Error("[reze] HTML bundle produced no adapter chunk");
    const workerFile = join(tempDir, "reze-ssg-worker.mjs");
    writeFileSync(workerFile, workerEntrySource(`./${adapterChunk}`));
    const discovery = await runWorker<DiscoverResult>(workerFile, "discover", {
      paths,
      base: input.base,
      assets: pageAssetUrls(registry, input.base, "/"),
      headDefaults,
      trailingSlash: options.trailingSlash,
      rootId: options.rootId,
      buildId,
      timeoutMs: options.timeoutMs,
      modules: input.modules,
    }, options.timeoutMs);
    assertDiscovery(discovery);
    if (discovery.mode !== mode.kind) {
      throw new Error(`[reze] SSG entry mode changed between analysis (${mode.kind}) and discovery (${discovery.mode})`);
    }
    const urls = discovery.mode === "standalone" ? ["/"] : discovery.urls.map((entry) => normalizePageUrl(entry.url));
    const canonical = urls.map((url) => canonicalPageUrl(url, options.trailingSlash));
    const planned = planOutputs(canonical);
    checkPublicCollisions(input.publicDir, [...planned.keys()]);
    const leafByUrl = new Map(canonical.map((url, index) => [url, discovery.mode === "standalone" ? "root" : discovery.urls[index]!.leafId]));
    const redirects = new Map<string, string>();
    for (const url of canonical) {
      const page = await runWorker<RenderResult>(workerFile, "render", {
        pathname: url,
        base: input.base,
        leafId: leafByUrl.get(url),
        rootId: options.rootId,
        buildId,
        timeoutMs: options.timeoutMs,
        assets: pageAssetUrls(registry, input.base, url),
        headDefaults,
        modules: input.modules,
      }, options.timeoutMs).catch((error: Error) => {
        throw new Error(`[reze] SSG render of ${JSON.stringify(url)} failed: ${error.message}`);
      });
      assertPage(url, page);
      if (page.status === "redirect") {
        const target = redirectPageTarget(page.to ?? "/", input.base, options);
        if (target !== undefined) redirects.set(url, target);
      }
      writePage(input, registry, captured, headDefaults, options, url, page, canonical);
    }
    assertNoRedirectCycles(redirects);
  } finally {
    removeTempDir(tempDir);
  }
}

function pageAssetUrls(registry: ClientRegistry, base: string, url: string): Record<string, string> {
  const depth = pageDepth(url);
  const out: Record<string, string> = {};
  for (const entry of registry.assetEntries()) {
    out[entry.id] = joinBase(base, entry.file, depth);
  }
  return out;
}

function pageHeadAssets(registry: ClientRegistry, base: string, url: string, modules: readonly string[], moduleFiles: ReadonlyMap<string, string>, bootstrapFile: string): { css: string[]; js: string[] } {
  const depth = pageDepth(url);
  const seeds = new Set<string>();
  for (const module of modules) {
    const file = moduleFiles.get(module);
    const chunk = file === undefined ? undefined : registry.chunkFileForModule(file);
    if (chunk !== undefined) seeds.add(chunk);
  }
  const closure = registry.staticClosure([...seeds]);
  return {
    css: closure.css.map(file => joinBase(base, file, depth)),
    js: closure.js.filter(file => file !== bootstrapFile).map(file => joinBase(base, file, depth)),
  };
}

function checkPublicCollisions(publicDir: string, files: readonly string[]): void {
  if (!existsSync(publicDir)) return;
  const seen: string[] = [];
  const walk = (current: string, prefix: string): void => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const rel = prefix === "" ? entry.name : `${prefix}/${entry.name}`;
      if (entry.isDirectory()) walk(join(current, entry.name), rel);
      else seen.push(rel);
    }
  };
  walk(publicDir, "");
  for (const file of files) {
    if (seen.includes(file)) {
      throw new Error(`[reze] SSG output ${JSON.stringify(file)} collides with a public directory file`);
    }
  }
}

function writePage(
  input: SsgBuildInput,
  registry: ClientRegistry,
  captured: CapturedState,
  headDefaults: { title?: string; description?: string; canonical?: string; robots?: string },
  options: ResolvedSsgOptions,
  url: string,
  page: RenderResult,
  concrete: readonly string[],
): void {
  const outFile = join(input.outDir, outputFileFor(url));
  mkdirSync(outFile.slice(0, outFile.lastIndexOf("/")), { recursive: true });
  if (page.status === "redirect") {
    const to = page.to ?? "/";
    const target = redirectPageTarget(to, input.base, options);
    if (target !== undefined) {
      if (!concrete.includes(target)) {
        throw new Error(`[reze] redirect target ${JSON.stringify(to)} for ${JSON.stringify(url)} is not a generated SSG page`);
      }
    }
    const assetOrigin = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(input.base) || input.base.startsWith("//");
    const suffix = new URL(to, "https://reze.invalid/");
    const destination = target === undefined
      ? to
      : joinBase(assetOrigin ? "/" : input.base, target.slice(1), pageDepth(url)) + suffix.search + suffix.hash;
    writeFileSync(outFile, buildRedirectPage({
      templateHtml: captured.templateHtml,
      templateFile: captured.templateFile,
      base: input.base,
      pathname: url,
      baseline: headDefaults,
      canonical: destination,
      to: destination,
      replace: page.replace ?? false,
      rootId: options.rootId,
      redirectSrc: joinBase(input.base, captured.redirectFile, pageDepth(url)),
    }));
    return;
  }
  const bootstrapFile = registry.entryChunk(SsgClientId).fileName;
  writeFileSync(outFile, buildPage({
    templateHtml: captured.templateHtml,
    templateFile: captured.templateFile,
    baseline: headDefaults,
    rootId: options.rootId,
    base: input.base,
    pathname: url,
    metadata: page.metadata ?? {},
    content: page.html ?? "",
    payload: page.payload ?? "",
    portals: page.portals ?? [],
    assets: pageHeadAssets(registry, input.base, url, page.modules!, input.moduleFiles, bootstrapFile),
  }));
}

function redirectPageTarget(to: string, base: string, options: ResolvedSsgOptions): string | undefined {
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(to) || to.startsWith("//")) {
    if (!/^https?:/i.test(to) && !to.startsWith("//")) throw new Error(`[reze] unsupported redirect target ${JSON.stringify(to)}`);
    return undefined;
  }
  let pathname = new URL(to, "https://reze.invalid/").pathname;
  const prefix = base.startsWith("/") && !base.startsWith("//") ? base.replace(/\/$/, "") : "";
  if (prefix !== "") {
    if (pathname === prefix) pathname = "/";
    else if (pathname.startsWith(`${prefix}/`)) pathname = pathname.slice(prefix.length);
    else throw new Error(`[reze] redirect target ${JSON.stringify(to)} is outside the SSG base`);
  }
  return canonicalPageUrl(normalizePageUrl(pathname), options.trailingSlash);
}

function assertNoRedirectCycles(redirects: ReadonlyMap<string, string>): void {
  const states = new Map<string, 1 | 2>();
  for (const start of redirects.keys()) {
    if (states.has(start)) continue;
    const path: string[] = [];
    let current: string | undefined = start;
    while (current !== undefined && states.get(current) !== 2) {
      if (states.get(current) === 1) throw new Error(`[reze] redirect cycle: ${[...path, current].join(" -> ")}`);
      states.set(current, 1);
      path.push(current);
      current = redirects.get(current);
    }
    for (const node of path) states.set(node, 2);
  }
}
