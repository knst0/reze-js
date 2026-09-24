import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  createServer,
  type EnvironmentModuleNode,
  type HotUpdateOptions,
  type Plugin,
  type ViteDevServer,
} from "vite";
import { afterEach, expect, test, vi } from "vitest";

import reze, { type Options } from "../src/index";

type HotUpdate = (
  this: { environment: ViteDevServer["environments"][string] },
  options: HotUpdateOptions,
) => Promise<EnvironmentModuleNode[] | void>;

const Sources: Record<string, string> = {
  "index.html": `<!doctype html><html><body><script type="module" src="/main.tsx"></script></body></html>\n`,
  "main.tsx": `import { render } from "reze-js";\nimport { App } from "./App";\n\nrender(() => <App />, document.body);\n`,
  "App.tsx": `import { title } from "./state";\nimport { Label } from "./Label";\nimport { Title } from "./Title";\n\nexport function App() {\n  return (\n    <main>\n      <h1>{title()}</h1>\n      <Label />\n      <Title />\n    </main>\n  );\n}\n`,
  "state.ts": `import { signal } from "reze-js";\n\nexport const [title, setTitle] = signal("Reze");\n`,
  "Label.tsx": `export function Label() {\n  return <button onClick={() => {}}>label</button>;\n}\n`,
  "Title.tsx": `export function Title() {\n  return <h2>static</h2>;\n}\n`,
};

const Outside = `import { title } from "./state";\n\nexport const echo = title;\n`;

let dir: string | undefined;
let server: ViteDevServer | undefined;

afterEach(async () => {
  await server?.close();
  server = undefined;
  if (dir) {
    rmSync(dir, { recursive: true, force: true });
    dir = undefined;
  }
});

async function serve(
  options: Options,
  extra: Record<string, string> = {},
): Promise<{ app: string; plugin: Plugin }> {
  dir = mkdtempSync(join(tmpdir(), "reze-dev-"));
  const app = join(dir, "app");
  mkdirSync(app);
  const runtime = join(dir, "runtime", "index.js");
  mkdirSync(join(dir, "runtime"));
  writeFileSync(runtime, "export {};\n");
  for (const [name, source] of Object.entries({ ...Sources, ...extra }))
    writeFileSync(join(app, name), source);
  const plugin = reze(options);
  server = await createServer({
    root: app,
    configFile: false,
    logLevel: "silent",
    appType: "custom",
    plugins: [plugin],
    resolve: { alias: { "reze-js": runtime } },
    optimizeDeps: { noDiscovery: true, include: [] },
    server: { middlewareMode: true, ws: false, watch: null },
  });
  return { app, plugin };
}

async function transformed(url: string): Promise<string> {
  const out = await server!.environments.client.transformRequest(url);
  return out!.code;
}

async function hotUpdate(plugin: Plugin, file: string, source: string) {
  writeFileSync(file, source);
  const environment = server!.environments.client;
  const send = vi.spyOn(environment.hot, "send").mockImplementation(() => {});
  const modules = await (plugin.hotUpdate as HotUpdate).call(
    { environment },
    {
      type: "update",
      file,
      timestamp: Date.now(),
      modules: [...(environment.moduleGraph.getModulesByFile(file) ?? [])],
      read: () => source,
      server: server!,
    },
  );
  const payloads = send.mock.calls.map(([payload]) => payload as unknown);
  const reloaded = payloads.some(
    (payload) =>
      typeof payload === "object" &&
      payload !== null &&
      "type" in payload &&
      payload.type === "full-reload",
  );
  send.mockRestore();
  return { reloaded, files: modules ? new Set(modules.map((mod) => mod.file)) : undefined };
}

async function loadAll(): Promise<void> {
  for (const url of ["/main.tsx", "/App.tsx", "/state.ts", "/Label.tsx", "/Title.tsx"])
    await transformed(url);
}

test("without program.dev, serve compiles in module mode", async () => {
  const { app, plugin } = await serve({}, { "outside.ts": Outside });
  const code = await transformed("/App.tsx");
  expect(code).not.toContain("<h1>Reze</h1>");
  expect(await transformed("/outside.ts")).toContain("title");
  expect(
    (await hotUpdate(plugin, join(app, "Label.tsx"), Sources["Label.tsx"])).files,
  ).toBeUndefined();
});

test("program.dev compiles with program facts from the HTML entries", async () => {
  await serve({ program: { dev: true } });
  expect(await transformed("/App.tsx")).toContain("<h1>Reze</h1>");
});

test("editing a leaf updates it and its importers without a reload", async () => {
  const { app, plugin } = await serve({ program: { dev: true } });
  await loadAll();
  const update = await hotUpdate(
    plugin,
    join(app, "Label.tsx"),
    Sources["Label.tsx"].replace("label", "edited"),
  );
  expect(update.reloaded).toBe(false);
  expect(update.files).toEqual(
    new Set(["main.tsx", "App.tsx", "Label.tsx"].map((name) => join(app, name))),
  );
});

test("editing a closed module reloads the page and refolds its importers", async () => {
  const { app, plugin } = await serve({ program: { dev: true } });
  await loadAll();
  const update = await hotUpdate(
    plugin,
    join(app, "state.ts"),
    Sources["state.ts"].replace('"Reze"', '"Dev"'),
  );
  expect(update).toEqual({ reloaded: true, files: new Set() });
  expect(await transformed("/App.tsx")).toContain("<h1>Dev</h1>");
});

test("a component turning from static to client reloads the page", async () => {
  const { app, plugin } = await serve({ program: { dev: true } });
  await loadAll();
  const client = Sources["Title.tsx"].replace("<h2>", "<h2 onClick={() => {}}>");
  expect((await hotUpdate(plugin, join(app, "Title.tsx"), client)).reloaded).toBe(true);
});

test("a transform of an edited file relinks instead of reporting stale facts", async () => {
  const { app } = await serve({ program: { dev: true } });
  const file = join(app, "Title.tsx");
  await transformed("/Title.tsx");
  const edited = `// edited\n${Sources["Title.tsx"].replace("static", "fresh")}`;
  writeFileSync(file, edited);
  const out = await server!.environments.client.pluginContainer.transform(edited, file);
  expect(out.code).toContain("fresh");
});

test("a module outside the program importing a closed module fails its transform", async () => {
  await serve({ program: { dev: true, exclude: /outside/ } }, { "outside.ts": Outside });
  await expect(transformed("/outside.ts")).rejects.toThrow(/PROGRAM_OPEN_IMPORT/);
});
