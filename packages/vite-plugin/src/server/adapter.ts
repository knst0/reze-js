export const ClientRequest = "/@reze/client.js";
export const ClientId = "\0reze:client.js";
export const IslandsId = "\0reze:islands.js";
export const ServerEntryId = "\0reze:server-entry.js";
export const ServerFactsSpecifier = "reze-client.js";
export const DevFactsId = "\0reze:dev-facts.js";
export const SsgViewId = "\0reze:ssg-view.tsx";
export const SsgRedirectId = "\0reze:ssg-redirect.js";
export const HtmlEnv = "reze_html";
export const GeneratedIds: readonly string[] = [ClientId, IslandsId, ServerEntryId, SsgRedirectId, DevFactsId];

export function viewSource(): string {
  return `import { routes } from "virtual:reze-routes";
export { routes };
export function ssgView(props) {
  const Router = props.router;
  return <Router />;
}
`;
}

export function serverEntrySource(): string {
  const viewLiteral = JSON.stringify(SsgViewId);
  return String.raw`import facts from "${ServerFactsSpecifier}";
import { HtmlSession, renderStream } from "reze-js/internal/html";
import * as routerServer from "@rezejs/router/internal/server";
const externalBase = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(facts.base) || facts.base.startsWith("//");
const absoluteBase = facts.base.startsWith("/") && !facts.base.startsWith("//");
const basePath = absoluteBase ? facts.base.replace(/\/$/, "") : "";
let viewPromise;
function loadView() {
  viewPromise ??= (async () => {
    routerServer.installServerLinkTarget();
    return import(${viewLiteral});
  })();
  return viewPromise;
}
function stripBase(pathname) {
  if (basePath === "") return pathname;
  if (pathname !== basePath && !pathname.startsWith(basePath + "/")) return undefined;
  return pathname.slice(basePath.length) || "/";
}
function pageDepth(pathname) {
  return pathname.split("/").length - 2;
}
function joinBase(file, depth) {
  if (externalBase) return facts.base.replace(/\/+$/, "") + "/" + file;
  if (absoluteBase) return basePath + "/" + file;
  return (depth === 0 ? "./" : "../".repeat(depth)) + file;
}
function rebaser(depth) {
  const prefix = depth === 0 ? "./" : "../".repeat(depth);
  return (text) => text.replace(/(\s(?:src|href)=")\.\//g, "$1" + prefix);
}
function rebaseTemplate(parts, depth) {
  const rebase = rebaser(depth);
  return {
    beforeHeadEnd: rebase(parts.beforeHeadEnd),
    headEndToRoot: rebase(parts.headEndToRoot),
    rootEndToBodyEnd: rebase(parts.rootEndToBodyEnd),
    bodyEndToEnd: parts.bodyEndToEnd,
  };
}
function rebaseHeadDefaults(defaults, depth) {
  const rebase = rebaser(depth);
  return defaults.map((entry) => ({ key: entry.key, html: rebase(entry.html) }));
}
function clientModulesAt(depth) {
  const moduleAt = (moduleId) => {
    const entry = facts.modules[moduleId];
    if (entry === undefined) throw new Error("[reze] no client chunk for module " + moduleId);
    return entry;
  };
  return {
    url: (moduleId) => joinBase(moduleAt(moduleId).file, depth),
    css: (moduleId) => (facts.modules[moduleId]?.css ?? []).map((file) => joinBase(file, depth)),
    preload: (moduleId) => (facts.modules[moduleId]?.preload ?? []).map((file) => joinBase(file, depth)),
  };
}
function assetsAt(depth) {
  return Object.fromEntries(
    Object.entries(facts.assets).map(([id, file]) => [id, file.startsWith("data:") ? file : joinBase(file, depth)]),
  );
}
const AssetMarker = "\0reze-asset:";
function holdBackLength(text) {
  const start = text.lastIndexOf(AssetMarker);
  if (start !== -1 && text.indexOf("\0", start + AssetMarker.length) === -1) return text.length - start;
  for (let length = Math.min(AssetMarker.length - 1, text.length); length > 0; length--) {
    if (AssetMarker.startsWith(text.slice(text.length - length))) return length;
  }
  return 0;
}
function substituteAssets(body, assets) {
  if (Object.keys(assets).length === 0) return body;
  const replaceAll = (text) =>
    text.replace(/\0reze-asset:([^\0]*)\0/g, (_, id) => {
      if (!Object.hasOwn(assets, id)) throw new Error("[reze] unknown asset " + id);
      return assets[id];
    });
  const decoder = new TextDecoder();
  const encoder = new TextEncoder();
  let pending = "";
  return body.pipeThrough(
    new TransformStream({
      transform(chunk, controller) {
        pending += decoder.decode(chunk, { stream: true });
        const hold = holdBackLength(pending);
        const ready = pending.slice(0, pending.length - hold);
        pending = pending.slice(pending.length - hold);
        if (ready !== "") controller.enqueue(encoder.encode(replaceAll(ready)));
      },
      flush(controller) {
        pending += decoder.decode();
        if (pending !== "") controller.enqueue(encoder.encode(replaceAll(pending)));
      },
    }),
  );
}
function escapeHtml(text) {
  return text.replace(/["&<>\u2028\u2029]/g, (char) => "&#x" + char.charCodeAt(0).toString(16) + ";");
}
function jsonScript(value) {
  return JSON.stringify(value).replace(/[<>&\u2028\u2029]/g, (char) => "\\u" + char.charCodeAt(0).toString(16).padStart(4, "0"));
}
function pageHead(route) {
  const tags = ['<meta name="reze-build" content="' + escapeHtml(facts.buildId) + '">'];
  if (route !== undefined) tags.push('<script type="application/json" data-rz-route>' + jsonScript(route) + "</script>");
  return tags.join("\n");
}
function routeState(pathname, search, matches) {
  const params = Object.assign({}, ...matches.map((match) => match.params));
  return {
    pathname,
    search,
    hash: "",
    params,
    matches: matches.map((match) => ({ id: match.id, params: match.params })),
  };
}
function redirectResult(appPath, redirect) {
  const to = routerServer.resolveSsgRedirectTarget(appPath, redirect.to);
  return {
    status: "redirect",
    to: routerServer.isExternalRedirectTarget(to) ? to : basePath + to,
    replace: redirect.replace,
  };
}
async function respond(pathname, search, mode, signal, overrides) {
  const appPath = stripBase(pathname);
  if (appPath === undefined) return { status: "not-found" };
  const depth = pageDepth(pathname);
  const session = new HtmlSession({
    pathname,
    rootId: facts.rootId,
    buildId: facts.buildId,
    timeoutMs: facts.timeoutMs,
    streaming: mode === "stream",
  });
  let handedOff = false;
  try {
    const view = await session.load(loadView);
    const prepared = await session.load(() => routerServer.prepareServerRoute(view.routes, appPath, { base: basePath }));
    if (prepared.status === "redirect") return redirectResult(appPath, prepared);
    if (prepared.status === "not-found") return { status: "not-found" };
    const build = () => view.ssgView({ router: prepared.component });
    const route = routeState(appPath, search, prepared.matches);
    const template = overrides?.template ?? rebaseTemplate(facts.template, depth);
    const clientModules = overrides?.clientModules ?? clientModulesAt(depth);
    const headDefaults = overrides?.headDefaults ?? rebaseHeadDefaults(facts.headDefaults, depth);
    handedOff = true;
    const body = await renderStream({ build, session, template, head: pageHead(route), headDefaults, clientModules, mode, signal });
    const redirect = routerServer.takeServerRedirect(prepared.handle);
    if (redirect !== undefined) {
      await body.cancel();
      return redirectResult(appPath, redirect);
    }
    return { status: "render", body: substituteAssets(body, assetsAt(depth)) };
  } finally {
    if (!handedOff) session.dispose();
  }
}
function text(status, body) {
  return new Response(body, { status, headers: { "content-type": "text/plain; charset=utf-8" } });
}
const ModuleSrcScript = /<script\b[^>]*\btype="module"[^>]*\bsrc="[^"]*"[^>]*><\/script>/g;
function errorDocument(template, head) {
  const strip = (html) => html.replace(ModuleSrcScript, "");
  const html = strip(template.beforeHeadEnd) + head + strip(template.headEndToRoot) + "Internal Server Error" +
    strip(template.rootEndToBodyEnd) + template.bodyEndToEnd;
  return new Response(html, { status: 500, headers: { "content-type": "text/html; charset=utf-8" } });
}
export async function handler(request, overrides) {
  const url = new URL(request.url);
  try {
    const result = await respond(url.pathname, url.search, "stream", request.signal, overrides);
    if (result.status === "not-found") return text(404, "Not Found");
    if (result.status === "redirect") return new Response(null, { status: 302, headers: { location: result.to } });
    return new Response(result.body, { status: 200, headers: { "content-type": "text/html; charset=utf-8" } });
  } catch (error) {
    console.error(error);
    const depth = pageDepth(url.pathname);
    const template = overrides?.template ?? rebaseTemplate(facts.template, depth);
    const headDefaults = overrides?.headDefaults ?? rebaseHeadDefaults(facts.headDefaults, depth);
    const head = headDefaults.map((entry) => entry.html).join("") + pageHead(undefined);
    return errorDocument(template, head + (overrides?.errorHead(error) ?? ""));
  }
}
export async function prerender(input) {
  const result = await respond(basePath + input.pathname, "", "buffered", undefined, undefined);
  if (result.status === "redirect") return result;
  if (result.status === "not-found") throw new Error("[reze] SSG route " + input.pathname + " matched no route");
  return { status: "render", html: await new Response(result.body).text() };
}
export async function discover(input) {
  const session = new HtmlSession({
    pathname: "/",
    rootId: facts.rootId,
    buildId: facts.buildId,
    timeoutMs: facts.timeoutMs,
    streaming: false,
  });
  try {
    const view = await session.load(loadView);
    const descriptors = routerServer.describeSsgRoutes(view.routes);
    const urls = routerServer.enumerateSsgUrls(descriptors, input.paths, { trailingSlash: input.trailingSlash });
    return urls.map((entry) => entry.url);
  } finally {
    session.dispose();
  }
}
`;
}

export function clientEntrySource(rootId: string): string {
  return `import { attachStream } from "reze-js/internal/client";
import { installSwapNavigation } from "@rezejs/router/internal/swap";
import islands from ${JSON.stringify(IslandsId)};
export { islands };
const swap = installSwapNavigation({ base: import.meta.env.BASE_URL, rootId: ${JSON.stringify(rootId)}, islands });
attachStream(document, document, { wrap: swap.wrap, islands });
`;
}

export function islandsSource(entries: readonly { moduleId: string; file: string }[]): string {
  const lines = entries.map(({ moduleId, file }) => `  ${JSON.stringify(moduleId)}: () => import(${JSON.stringify(file)}),`);
  return `export default {\n${lines.join("\n")}\n};\n`;
}

export function devFactsSource(options: { base: string; rootId: string; timeoutMs: number }): string {
  const facts = {
    buildId: "dev",
    base: options.base,
    rootId: options.rootId,
    timeoutMs: options.timeoutMs,
    template: { beforeHeadEnd: "", headEndToRoot: "", rootEndToBodyEnd: "", bodyEndToEnd: "" },
    headDefaults: [],
    assets: {},
    modules: {},
  };
  return `export default ${JSON.stringify(facts)};\n`;
}

export function redirectModuleSource(): string {
  return `const payload = document.querySelector("script[data-reze-redirect]")?.textContent;
const target = payload === undefined || payload === null || payload === "" ? undefined : JSON.parse(payload);
if (target !== undefined) {
  if (target.replace === true) location.replace(target.to);
  else location.assign(target.to);
}
`;
}

export function workerEntrySource(adapterFile: string): string {
  return `import { parentPort, workerData } from "node:worker_threads";
import * as adapter from ${JSON.stringify(adapterFile)};
const report = (message) => parentPort.postMessage(message);
let current;
try {
  const urls = await adapter.discover(workerData.input);
  const pages = [];
  for (const url of urls) {
    current = url;
    pages.push({ pathname: url, result: await adapter.prerender({ pathname: url }) });
    report({ ok: true, progress: url });
  }
  report({ ok: true, result: pages });
} catch (error) {
  const detail = error instanceof Error ? error.message : String(error);
  const message = current === undefined ? detail : "[reze] SSG render of " + current + " failed: " + detail;
  report({ ok: false, error: { message, stack: error instanceof Error ? error.stack : undefined } });
}
`;
}
