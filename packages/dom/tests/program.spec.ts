import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";

import { cleanup, fire, tick } from "@rezejs/test-utils";
import { build } from "vite";
import { afterEach, beforeAll, expect, test } from "vitest";

import reze from "@rezejs/vite-plugin";

afterEach(cleanup);

const programRoot = join(import.meta.dirname, "program");
const generated = join(import.meta.dirname, ".generated", "program");
const diagnosticsDir = join(generated, "diagnostics");

interface ServerBundle {
  renderPage: () => string;
  renderFn: () => string;
}

interface ClientBundle {
  hydratePage: (el: HTMLElement) => () => void;
}

interface Configuration {
  program: boolean;
  islands?: boolean;
  hydratable?: boolean;
  optimize?: boolean;
}

const configurations: Record<string, Configuration> = {
  islands: { program: true, islands: true, hydratable: true },
  plain: { program: true },
  nofacts: { program: false },
  noopt: { program: true, optimize: false },
};

type BuildName = keyof typeof configurations;

async function viteBuild(name: string, configuration: Configuration, server: boolean): Promise<void> {
  const outDir = join(generated, `${name}-${server ? "server" : "client"}`);
  mkdirSync(outDir, { recursive: true });
  await build({
    root: programRoot,
    logLevel: "silent",
    configFile: false,
    plugins: [
      reze({
        moduleName: "@rezejs/dom",
        ...configuration,
        diagnostics: { jsonl: join(diagnosticsDir, `${name}-${server ? "server" : "client"}.jsonl`) },
      }),
    ],
    build: {
      ...(server ? { ssr: true } : {}),
      outDir,
      emptyOutDir: true,
      minify: false,
      rollupOptions: {
        input: join(programRoot, server ? "entry-server.tsx" : "entry-client.tsx"),
        preserveEntrySignatures: "strict",
        output: { format: "esm", entryFileNames: "bundle.mjs" },
      },
    },
  });
}

async function buildProgram(): Promise<void> {
  rmSync(diagnosticsDir, { recursive: true, force: true });
  for (const [name, configuration] of Object.entries(configurations)) {
    await viteBuild(name, configuration, true);
    await viteBuild(name, configuration, false);
  }
}

beforeAll(buildProgram, 180000);

async function loadBundles(name: BuildName, tag: string): Promise<{ server: ServerBundle; client: ClientBundle }> {
  const query = `?program-${tag}`;
  return {
    server: await import(/* @vite-ignore */ join(generated, `${name}-server`, "bundle.mjs") + query),
    client: await import(/* @vite-ignore */ join(generated, `${name}-client`, "bundle.mjs") + query),
  };
}

function diagnosticsOf(name: BuildName, side: "server" | "client"): Record<string, unknown>[] {
  const jsonl = readFileSync(join(diagnosticsDir, `${name}-${side}.jsonl`), "utf8");
  return jsonl
    .split("\n")
    .filter((line) => line.length > 0)
    .map((line) => JSON.parse(line) as Record<string, unknown>);
}

function infos(name: BuildName): string[] {
  return diagnosticsOf(name, "server")
    .filter((diagnostic) => diagnostic.severity === "info")
    .map((diagnostic) => `${diagnostic.code} ${JSON.stringify(diagnostic.data)}`);
}

function bundleText(name: BuildName): string {
  return readFileSync(join(generated, `${name}-server`, "bundle.mjs"), "utf8");
}

function withoutIslandMarkers(html: string): string {
  return html.replace(/<!--\$[^]*?-->|<!--\/\$-->/g, "");
}

function normalized(html: string): string {
  return withoutIslandMarkers(html)
    .replace(/<!--[[\]]-->/g, "")
    .replace(/<!---->/g, "")
    .replace(/ data-hk="[^"]*"/g, "");
}

function attach(html: string): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.appendChild(container);
  return container;
}

function tagSequence(el: Element): string {
  return [...el.querySelectorAll("*")].map((node) => node.tagName).join(",");
}

async function awaitBundleEffects(): Promise<void> {
  tick();
  const { promise, resolve } = Promise.withResolvers<void>();
  setTimeout(resolve, 0);
  await promise;
}

test("whole-program analysis folds the exported signal and unproxies the exported store", () => {
  const decisions = infos("islands");
  expect(decisions.some((line) => line.includes('"scope":"program"') && line.startsWith("SIGNAL_FOLDED"))).toBe(
    true,
  );
  expect(decisions.some((line) => line.includes('"scope":"program"') && line.startsWith("STORE_UNPROXIED"))).toBe(
    true,
  );
  expect(decisions.some((line) => line.startsWith("ISLAND"))).toBe(true);
  expect(decisions.some((line) => line.startsWith("STATIC_COMPONENT"))).toBe(true);

  const optimized = bundleText("islands");
  expect(optimized).toContain("profile$count");
  expect(optimized).not.toContain("store({");
  expect(optimized).not.toContain('signal("count:")');

  const unfactored = bundleText("nofacts");
  expect(unfactored).toContain("store({");
  expect(unfactored).toContain('signal("count:")');
  expect(unfactored).not.toContain("profile$count");

  const unoptimized = bundleText("noopt");
  expect(unoptimized).toContain("store({");
  const unoptimizedDecisions = infos("noopt");
  expect(unoptimizedDecisions.some((line) => line.startsWith("SIGNAL_FOLDED"))).toBe(false);
  expect(unoptimizedDecisions.some((line) => line.startsWith("STORE_UNPROXIED"))).toBe(false);
});

test("island markers are the only server difference from the same page without islands", async () => {
  const islands = await loadBundles("islands", "markers");
  const plain = await loadBundles("plain", "markers");
  const withIslands = islands.server.renderPage();
  expect(withIslands).toContain("<!--$");
  expect(withoutIslandMarkers(withIslands)).toBe(plain.server.renderPage());
  expect(plain.server.renderPage()).not.toContain("<!--$");
});

test("builds without facts and without optimization render the same server page", async () => {
  const nofacts = await loadBundles("nofacts", "server");
  const noopt = await loadBundles("noopt", "server");
  expect(nofacts.server.renderPage()).toBe(noopt.server.renderPage());
  expect(nofacts.server.renderPage()).not.toContain("<!--$");
});

test("island hydration matches full hydration after every click", async () => {
  const islands = await loadBundles("islands", "hydrate");
  const plain = await loadBundles("plain", "hydrate");
  const islandContainer = attach(islands.server.renderPage());
  const islandDispose = islands.client.hydratePage(islandContainer);
  const fullContainer = attach(plain.server.renderPage());
  const fullDispose = plain.client.hydratePage(fullContainer);
  try {
    await awaitBundleEffects();
    expect(normalized(islandContainer.innerHTML)).toBe(normalized(fullContainer.innerHTML));
    for (let click = 0; click < 3; click++) {
      fire(islandContainer.querySelector("button")!, "click");
      fire(fullContainer.querySelector("button")!, "click");
      await awaitBundleEffects();
      expect(normalized(islandContainer.innerHTML)).toBe(normalized(fullContainer.innerHTML));
    }
    expect(islandContainer.textContent).toContain("4");
    expect(islandContainer.textContent).toContain("3");
  } finally {
    islandDispose();
    fullDispose();
  }
});

test("builds without facts and without optimization reach the same DOM after every click", async () => {
  const names: BuildName[] = ["islands", "plain", "nofacts", "noopt"];
  const bundles = await Promise.all(names.map((name) => loadBundles(name, "converge")));
  const containers = bundles.map((bundle) => attach(bundle.server.renderPage()));
  const disposers = bundles.map((bundle, index) => bundle.client.hydratePage(containers[index]!));
  try {
    await awaitBundleEffects();
    for (let click = 0; click <= 3; click++) {
      if (click > 0) {
        for (const container of containers) {
          fire(container.querySelector("button")!, "click");
        }
        await awaitBundleEffects();
      }
      const markup = containers.map((container) => normalized(container.innerHTML));
      const text = containers.map((container) => container.textContent ?? "");
      const tags = containers.map((container) => tagSequence(container));
      for (let other = 1; other < names.length; other++) {
        expect(text[other]).toBe(text[0]);
        expect(tags[other]).toBe(tags[0]);
        expect(markup[other]).toBe(markup[0]);
      }
    }
  } finally {
    for (const dispose of disposers) dispose();
  }
});

test("a function-valued island prop throws [ISLAND_PROPS] at renderToString", async () => {
  for (const name of Object.keys(configurations)) {
    const bundle = await loadBundles(name as BuildName, "island-props");
    expect(() => bundle.server.renderFn()).toThrowError("[ISLAND_PROPS]");
  }
});
