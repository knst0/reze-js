import type { AppMode } from "./export-graph";

export const SsgClientRequest = "/@reze/ssg-client.js";
export const SsgClientId = "\0reze:ssg-client.js";
export const SsgViewId = "\0reze:ssg-view.tsx";
export const SsgHtmlAdapterId = "\0reze:ssg-html-adapter.js";
export const SsgRedirectId = "\0reze:ssg-redirect.js";
export function viewSource(mode: AppMode, entrySpecifier: string): string {
  const entry = JSON.stringify(entrySpecifier);
  if (mode.kind === "standalone") {
    return `import App from ${entry};
export function SsgView() {
  return <App />;
}
`;
  }
  const shellImport = mode.hasShell ? `import Shell from ${entry};\n` : "";
  const shellExport = mode.hasShell
    ? `export function SsgShell(props) {
  return <Shell>{props.children}</Shell>;
}
`
    : "";
  return `import { createBrowserHistory, createRouter } from "@rezejs/router";
import { routes } from ${entry};
export { routes };
${shellImport}${shellExport}export function SsgView(props = {}) {
  const Router = props.router ?? createRouter({ routes, history: createBrowserHistory(import.meta.env.BASE_URL) });
  return ${mode.hasShell ? "<Router root={SsgShell} />" : "<Router />"};
}
`;
}

export function clientBootSource(options: { mode: AppMode; rootId: string }): string {
  const routerImport = options.mode.kind === "router"
    ? `import { prepareHydratedRouter } from "@rezejs/router/internal/hydrate";\n`
    : "";
  const mount = options.mode.kind === "router"
    ? `const router = await session.load(() => prepareHydratedRouter(view.routes, session, session.base));\nawait hydrate(() => view.SsgView({ router }), root);`
    : "await hydrate(() => view.SsgView(), root);";
  return `import { prepareHydration } from "reze-js/internal/hydrate";
import { hydrate } from "reze-js";
${routerImport}
const root = document.getElementById(${JSON.stringify(options.rootId)});
if (root === null) throw new Error("[reze] SSG mount element is missing");
const session = prepareHydration(root, { bootstrapUrl: import.meta.url, base: import.meta.env.BASE_URL });
try {
/* The generated view must evaluate after scope registration; a static import runs too early. */
const view = await session.load(() => import(${JSON.stringify(SsgViewId)}));
${mount}
} catch (error) {
  try { session.dispose(); }
  catch (cleanupError) { throw new AggregateError([error, cleanupError], "hydration and cleanup failed"); }
  throw error;
}
`;
}

export function devBootSource(rootId: string): string {
  return `import { render } from "reze-js";
import { SsgView } from ${JSON.stringify(SsgViewId)};
const root = document.getElementById(${JSON.stringify(rootId)});
if (root === null) throw new Error("[reze] SSG mount element is missing");
render(() => SsgView(), root);
`;
}

// The adapter dynamic-imports the view and entry after the HtmlSession and
// the asset registry exist, so module initializers run under the page scope
// with asset lookups available. The router import stays inside the router
// branch so standalone apps never bundle it.
export function htmlAdapterSource(options: { mode: AppMode; entrySpecifier: string }): string {
  const entry = JSON.stringify(options.entrySpecifier);
  const viewId = JSON.stringify(SsgViewId);
  const prelude = `function portalEntry(session, portal) {
  const token = portal.node.token;
  if (typeof token !== "string" || token === "") throw new Error("[reze] portal node has no token for hydration claim");
  if (!session.ranges.has(token)) throw new Error("[reze] portal token is unknown to the session: " + JSON.stringify(token));
  return { placement: portal.placement, token, html: serializePortalNodes([portal.node]) };
}
async function finishRender(session, tree, input, metadata) {
  await session.settle();
  const portals = [];
  const nodes = [];
  for (const portal of session.portals) {
    if (portal.instance.retired) continue;
    portals.push(portalEntry(session, portal));
    nodes.push(portal.node);
  }
  const layout = describeNodes([tree, ...nodes], { resolver: { rangeInfo: (range) => session.ranges.get(range.token) } });
  const payload = session.snapshot(layout);
  return {
    status: "render",
    html: serializeNodes([tree]),
    portals,
    layout,
    payload: serializePayload(payload),
    modules: payload.modules,
    metadata,
  };
}
function createSession(input) {
  return new HtmlSession({
    pathname: (input.base?.startsWith("/") && !input.base.startsWith("//") ? input.base.replace(/\\/$/, "") : "") + input.pathname,
    rootId: input.rootId,
    buildId: input.buildId,
    timeoutMs: input.timeoutMs,
    headDefaults: input.headDefaults,
    modules: [...input.modules],
  });
}`;
  if (options.mode.kind === "standalone") {
    return `import { HtmlSession, describeNodes, hMount, installHtmlAssets, serializeNodes, serializePortalNodes, serializePayload } from "reze-js/internal/html";
${prelude}
export function discover() {
  return { mode: "standalone", descriptors: [], urls: [{ url: "/", leafId: "root", params: {} }] };
}
export async function renderPage(input) {
  const session = createSession(input);
  const disposeRegistry = installHtmlAssets({ ...input.assets });
  try {
    /* Application evaluation requires the page scope and asset registry. */
    const view = await session.load(() => import(${viewId}));
    const tree = session.run(() => hMount(() => view.SsgView()));
    return await finishRender(session, tree, input, {});
  } finally {
    try { session.dispose(); } finally { disposeRegistry(); }
  }
}
`;
  }
  return `import { HtmlSession, describeNodes, hMount, installHtmlAssets, serializeNodes, serializePortalNodes, serializePayload } from "reze-js/internal/html";
import * as routerSsg from "@rezejs/router/internal/ssg";
${prelude}
function redirectPage(pathname, base, redirect) {
  const to = routerSsg.resolveSsgRedirectTarget(pathname, redirect.to);
  return { status: "redirect", to: routerSsg.isExternalRedirectTarget(to) ? to : base + to, replace: redirect.replace };
}
export async function discover(input) {
  routerSsg.installSsgLinkTarget();
  const session = createSession({ ...input, pathname: "/" });
  const disposeRegistry = installHtmlAssets({ ...input.assets });
  try {
    /* Application evaluation requires the discovery scope and asset registry. */
    const entry = await session.load(() => import(${entry}));
    if (entry.routes === undefined) throw new Error("[reze] SSG entry analyzed as router app but exports no routes");
    const descriptors = routerSsg.describeSsgRoutes(entry.routes);
    const urls = routerSsg.enumerateSsgUrls(descriptors, input.paths, { trailingSlash: input.trailingSlash });
    return { mode: "router", descriptors, urls };
  } finally {
    try { session.dispose(); } finally { disposeRegistry(); }
  }
}
export async function renderPage(input) {
  const session = createSession(input);
  const base = input.base?.startsWith("/") && !input.base.startsWith("//") ? input.base.replace(/\\/$/, "") : "";
  const disposeRegistry = installHtmlAssets({ ...input.assets });
  try {
    routerSsg.installSsgLinkTarget();
    /* Application evaluation requires the page scope and asset registry. */
    const view = await session.load(() => import(${viewId}));
    const entry = await session.load(() => import(${entry}));
    const prepared = await session.load(() => routerSsg.prepareSsgRoute(entry.routes, input.pathname, { base }));
    if (prepared.status === "redirect") {
      return redirectPage(input.pathname, base, prepared);
    }
    if (prepared.matches.at(-1)?.id !== input.leafId) throw new Error("[reze] enumerated route loses matching priority: " + input.leafId);
    session.run(() => {
      for (const match of prepared.matches) session.recordRoute(match.id, match.params, match.hasData, match.data);
    });
    const tree = session.run(() => hMount(() => view.SsgView({ router: prepared.component })));
    await session.settle();
    const redirect = routerSsg.takeSsgRedirect(prepared.handle);
    if (redirect !== undefined) return redirectPage(input.pathname, base, redirect);
    return await finishRender(session, tree, input, prepared.metadata);
  } finally {
    try { session.dispose(); } finally { disposeRegistry(); }
  }
}
`;
}

export function redirectModuleSource(): string {
  return `const root = document.currentScript?.previousElementSibling;
const payload = root?.textContent;
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
const input = workerData.input;
try {
  const result = workerData.kind === "discover" ? await adapter.discover(input) : await adapter.renderPage(input);
  parentPort.postMessage({ ok: true, result });
} catch (error) {
  parentPort.postMessage({ ok: false, error: error instanceof Error ? { message: error.message, stack: error.stack } : String(error) });
}
`;
}
